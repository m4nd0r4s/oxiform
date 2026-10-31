//! Python objects the bridge looks up once and keeps.

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyType;

static VALIDATION_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static DECODE_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static SCHEMA_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static DESCRIBE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
static ANY: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

pub(crate) fn validation_error(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    VALIDATION_ERROR.import(py, "oxiform.errors", "ValidationError")
}

pub(crate) fn decode_error(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    DECODE_ERROR.import(py, "oxiform.errors", "DecodeError")
}

pub(crate) fn schema_error(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    SCHEMA_ERROR.import(py, "oxiform.errors", "SchemaError")
}

/// `oxiform._introspection.describe`.
pub(crate) fn describe(py: Python<'_>) -> PyResult<&Bound<'_, PyAny>> {
    DESCRIBE.import(py, "oxiform._introspection", "describe")
}

/// `typing.Any`, the default target type.
pub(crate) fn any_type(py: Python<'_>) -> PyResult<&Bound<'_, PyAny>> {
    ANY.import(py, "typing", "Any")
}
