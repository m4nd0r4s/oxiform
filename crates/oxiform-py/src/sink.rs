//! Builds Python objects from validated values.

use oxiform_core::plan::StructId;
use oxiform_core::sink::Sink;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString};
use pyo3::{ffi, intern};

use crate::compiled::CompiledType;
use crate::description::{FieldDefault, FieldSetter};

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

    fn list(&mut self, items: Vec<Self::Value>) -> PyResult<Self::Value> {
        Ok(PyList::new(self.py, items)?.into_any())
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
        let py = self.py;
        let runtime = &self.compiled.structs[structure.index()];
        let cls = runtime.cls.bind(py);
        // SAFETY: `cls` is a valid type object. Allocating without calling `__new__` or
        // `__init__` is how the instance gets its fields directly; the result is checked.
        let instance = unsafe {
            Bound::from_owned_ptr_or_err(py, ffi::PyType_GenericAlloc(cls.as_type_ptr(), 0))?
        };
        for ((setter, default), value) in runtime.setters.iter().zip(&runtime.defaults).zip(fields)
        {
            let value = match (value.take(), default) {
                (Some(value), _) => value,
                (None, FieldDefault::Value(value)) => value.bind(py).clone(),
                (None, FieldDefault::Factory(factory)) => factory.bind(py).call0()?,
                (None, FieldDefault::Required) => {
                    unreachable!("the executor reports missing required fields")
                }
            };
            set_field(&instance, setter, value)?;
        }
        if runtime.has_post_init {
            instance.call_method0(intern!(py, "__post_init__"))?;
        }
        Ok(instance)
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
