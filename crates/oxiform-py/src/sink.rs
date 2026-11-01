//! Builds Python objects from validated values.

use oxiform_core::plan::{ChoiceId, ConversionId, StructId};
use oxiform_core::sink::Sink;
use oxiform_core::temporal::Temporal;
use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDate, PyDateTime, PyDelta, PyDict, PyFloat, PyFrozenSet, PyInt,
    PyList, PySet, PyString, PyTime, PyTuple, PyTzInfo,
};
use pyo3::{ffi, intern};

use crate::compiled::CompiledType;
use crate::description::{FieldDefault, FieldSetter, Layout, MemberSource};

pub(crate) struct PythonSink<'run, 'py> {
    pub(crate) py: Python<'py>,
    pub(crate) compiled: &'run CompiledType,
}

impl<'py> Sink for PythonSink<'_, 'py> {
    type Value = Bound<'py, PyAny>;
    type Error = PyErr;
    type Map = Bound<'py, PyDict>;

    fn none(&mut self) -> PyResult<Self::Value> {
        Ok(self.py.None().into_bound(self.py))
    }

    fn bool(&mut self, value: bool) -> PyResult<Self::Value> {
        Ok(PyBool::new(self.py, value).to_owned().into_any())
    }

    fn integer(&mut self, value: i64) -> PyResult<Self::Value> {
        Ok(value.into_pyobject(self.py)?.into_any())
    }

    fn big_integer(&mut self, digits: &str) -> PyResult<Self::Value> {
        self.py.get_type::<PyInt>().call1((digits,))
    }

    fn float(&mut self, value: f64) -> PyResult<Self::Value> {
        Ok(PyFloat::new(self.py, value).into_any())
    }

    fn string(&mut self, value: &str) -> PyResult<Self::Value> {
        new_string(self.py, value)
    }

    fn bytes(&mut self, value: &[u8], mutable: bool) -> PyResult<Self::Value> {
        if mutable {
            Ok(PyByteArray::new(self.py, value).into_any())
        } else {
            Ok(PyBytes::new(self.py, value).into_any())
        }
    }

    fn temporal(&mut self, value: Temporal) -> PyResult<Self::Value> {
        let py = self.py;
        Ok(match value {
            Temporal::DateTime(date, time) => PyDateTime::new(
                py,
                i32::from(date.year),
                date.month,
                date.day,
                time.hour,
                time.minute,
                time.second,
                time.microsecond,
                time_zone(py, time.offset)?.as_ref(),
            )?
            .into_any(),
            Temporal::Date(date) => {
                PyDate::new(py, i32::from(date.year), date.month, date.day)?.into_any()
            }
            Temporal::Time(time) => PyTime::new(
                py,
                time.hour,
                time.minute,
                time.second,
                time.microsecond,
                time_zone(py, time.offset)?.as_ref(),
            )?
            .into_any(),
            Temporal::Duration(duration) => PyDelta::new(
                py,
                duration.days,
                duration.seconds,
                duration.microseconds,
                false,
            )?
            .into_any(),
        })
    }

    /// Calls the conversion with `raw`. `ValueError` and `KeyError` (which `ZoneInfo` raises
    /// for an unknown zone) reject the value; any other exception propagates.
    fn convert(
        &mut self,
        conversion: ConversionId,
        raw: Self::Value,
    ) -> PyResult<Option<Self::Value>> {
        let convert = self.compiled.runtime.conversions[conversion.index()].bind(self.py);
        match convert.call1((raw,)) {
            Ok(value) => Ok(Some(value)),
            Err(error)
                if error.is_instance_of::<PyValueError>(self.py)
                    || error.is_instance_of::<PyKeyError>(self.py) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    fn list(&mut self, items: Vec<Self::Value>) -> PyResult<Self::Value> {
        Ok(PyList::new(self.py, items)?.into_any())
    }

    fn tuple(&mut self, items: Vec<Self::Value>) -> PyResult<Self::Value> {
        Ok(PyTuple::new(self.py, items)?.into_any())
    }

    fn set(&mut self, items: Vec<Self::Value>, frozen: bool) -> PyResult<Self::Value> {
        if frozen {
            Ok(PyFrozenSet::new(self.py, items)?.into_any())
        } else {
            Ok(PySet::new(self.py, items)?.into_any())
        }
    }

    fn choice(&mut self, choice: ChoiceId, index: u32) -> PyResult<Self::Value> {
        let objects = &self.compiled.runtime.choices[choice.index()];
        Ok(objects[index as usize].bind(self.py).clone())
    }

    fn map_new(&mut self) -> PyResult<Self::Map> {
        Ok(PyDict::new(self.py))
    }

    fn map_insert(&mut self, map: &mut Self::Map, key: &str, value: Self::Value) -> PyResult<bool> {
        let size = map.len();
        map.set_item(new_string(self.py, key)?, value)?;
        Ok(map.len() > size)
    }

    fn map_finish(&mut self, map: Self::Map) -> PyResult<Self::Value> {
        Ok(map.into_any())
    }

    fn instance(
        &mut self,
        structure: StructId,
        fields: &mut [Option<Self::Value>],
    ) -> PyResult<Self::Value> {
        let runtime = &self.compiled.runtime.structs[structure.index()];
        build(self.py, &runtime.layout, &runtime.defaults, fields)
    }
}

/// `timezone.utc` for a zero offset, a fixed `timezone` for any other, `None` for none.
fn time_zone(py: Python<'_>, offset: Option<i32>) -> PyResult<Option<Bound<'_, PyTzInfo>>> {
    match offset {
        None => Ok(None),
        Some(0) => Ok(Some(PyTzInfo::utc(py)?.to_owned())),
        Some(seconds) => {
            let delta = PyDelta::new(py, 0, seconds, 0, true)?;
            Ok(Some(PyTzInfo::fixed_offset(py, delta)?))
        }
    }
}

/// Creates an instance as `layout` says, taking the values of its fields out of `fields`.
fn build<'py>(
    py: Python<'py>,
    layout: &Layout,
    defaults: &[FieldDefault],
    fields: &mut [Option<Bound<'py, PyAny>>],
) -> PyResult<Bound<'py, PyAny>> {
    let cls = layout.cls.bind(py);
    // SAFETY: `cls` is a valid type object. Allocating without calling `__new__` or
    // `__init__` is how the instance gets its fields directly; the result is checked.
    let instance = unsafe {
        Bound::from_owned_ptr_or_err(py, ffi::PyType_GenericAlloc(cls.as_type_ptr(), 0))?
    };
    for member in &layout.members {
        let value = match &member.source {
            MemberSource::Field(index) => match fields[*index].take() {
                Some(value) => value,
                None => default_value(py, &defaults[*index])?,
            },
            MemberSource::Default(default) => default_value(py, default)?,
            MemberSource::Flatten(inner) => build(py, inner, defaults, fields)?,
        };
        set_field(&instance, &member.setter, value)?;
    }
    if layout.has_post_init {
        instance.call_method0(intern!(py, "__post_init__"))?;
    }
    Ok(instance)
}

fn default_value<'py>(py: Python<'py>, default: &FieldDefault) -> PyResult<Bound<'py, PyAny>> {
    match default {
        FieldDefault::Value(value) => Ok(value.bind(py).clone()),
        FieldDefault::Factory(factory) => factory.bind(py).call0(),
        FieldDefault::Required => unreachable!("the executor reports missing required fields"),
    }
}

fn set_field(
    instance: &Bound<'_, PyAny>,
    setter: &FieldSetter,
    value: Bound<'_, PyAny>,
) -> PyResult<()> {
    match setter {
        FieldSetter::Slot(offset) => {
            // SAFETY: `offset` is the offset of an object slot of this exact class, read from
            // its member descriptor, and the instance was just allocated with every slot NULL.
            // The slot takes over the reference.
            unsafe {
                let slot = instance
                    .as_ptr()
                    .cast::<u8>()
                    .offset(*offset)
                    .cast::<*mut ffi::PyObject>();
                let previous = std::mem::replace(&mut *slot, value.into_ptr());
                ffi::Py_XDECREF(previous);
            }
            Ok(())
        }
        FieldSetter::Attribute(name) => {
            // SAFETY: all three pointers are valid for the duration of the call. The generic
            // setter bypasses a frozen dataclass's `__setattr__`, as its own `__init__` does.
            let status = unsafe {
                ffi::PyObject_GenericSetAttr(instance.as_ptr(), name.as_ptr(), value.as_ptr())
            };
            if status != 0 {
                return Err(PyErr::fetch(instance.py()));
            }
            Ok(())
        }
    }
}

/// A Python `str` from text that is already valid UTF-8. ASCII text, the common case, is copied
/// straight into a new compact string instead of being decoded again.
fn new_string<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyAny>> {
    if !text.is_ascii() {
        return Ok(PyString::new(py, text).into_any());
    }
    let length =
        ffi::Py_ssize_t::try_from(text.len()).expect("strings are smaller than isize::MAX");
    // SAFETY: `PyUnicode_New` with a maximum character of 127 returns a compact ASCII string
    // with room for `length` bytes, or NULL with an exception set. ASCII text is valid there
    // byte for byte.
    unsafe {
        let string = Bound::from_owned_ptr_or_err(py, ffi::PyUnicode_New(length, 127))?;
        let data = ffi::PyUnicode_DATA(string.as_ptr()).cast::<u8>();
        std::ptr::copy_nonoverlapping(text.as_ptr(), data, text.len());
        Ok(string)
    }
}
