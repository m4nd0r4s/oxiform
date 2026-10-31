//! Builds Python objects from validated values.

use oxiform_core::plan::StructId;
use oxiform_core::sink::Sink;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString};
use pyo3::{ffi, intern};

use crate::compiled::CompiledType;
use crate::description::FieldDefault;

pub(crate) struct PythonSink<'run, 'py> {
    pub(crate) py: Python<'py>,
    pub(crate) compiled: &'run CompiledType,
}

impl<'py> Sink for PythonSink<'_, 'py> {
    type Value = Bound<'py, PyAny>;
    type Error = PyErr;

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
        Ok(PyString::new(self.py, value).into_any())
    }

    fn list(&mut self, items: Vec<Self::Value>) -> PyResult<Self::Value> {
        Ok(PyList::new(self.py, items)?.into_any())
    }

    fn dict(&mut self, entries: Vec<(Self::Value, Self::Value)>) -> PyResult<Self::Value> {
        let dict = PyDict::new(self.py);
        for (key, value) in entries {
            dict.set_item(key, value)?;
        }
        Ok(dict.into_any())
    }

    fn instance(
        &mut self,
        structure: StructId,
        fields: Vec<Option<Self::Value>>,
    ) -> PyResult<Self::Value> {
        let py = self.py;
        let runtime = &self.compiled.structs[structure.index()];
        let cls = runtime.cls.bind(py);
        // SAFETY: `cls` is a valid type object. Allocating without calling `__new__` or
        // `__init__` is how the instance gets its fields directly; the result is checked.
        let instance = unsafe {
            Bound::from_owned_ptr_or_err(py, ffi::PyType_GenericAlloc(cls.as_type_ptr(), 0))?
        };
        for ((name, default), value) in runtime
            .field_names
            .iter()
            .zip(&runtime.defaults)
            .zip(fields)
        {
            let value = match (value, default) {
                (Some(value), _) => value,
                (None, FieldDefault::Value(value)) => value.bind(py).clone(),
                (None, FieldDefault::Factory(factory)) => factory.bind(py).call0()?,
                (None, FieldDefault::Required) => {
                    unreachable!("the executor reports missing required fields")
                }
            };
            // SAFETY: all three pointers are valid for the duration of the call. The generic
            // setter bypasses a frozen dataclass's `__setattr__`, as its own `__init__` does.
            let status = unsafe {
                ffi::PyObject_GenericSetAttr(instance.as_ptr(), name.as_ptr(), value.as_ptr())
            };
            if status != 0 {
                return Err(PyErr::fetch(py));
            }
        }
        if runtime.has_post_init {
            instance.call_method0(intern!(py, "__post_init__"))?;
        }
        Ok(instance)
    }
}
