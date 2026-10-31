//! `ox.json.encode` and `ox.json.Encoder`.

use oxiform_formats::json::JsonWriter;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use super::ObjectWalker;

/// Writes `obj` as JSON: compact, or indented by `indent` spaces; keys sorted with `sort_keys`.
#[pyfunction]
#[pyo3(signature = (obj, *, indent = None, sort_keys = false))]
pub(crate) fn json_encode<'py>(
    obj: &Bound<'py, PyAny>,
    indent: Option<usize>,
    sort_keys: bool,
) -> PyResult<Bound<'py, PyBytes>> {
    encode(obj, indent, sort_keys)
}

/// A reusable JSON encoder with fixed options.
#[pyclass(frozen, name = "JsonEncoder", module = "oxiform._oxiform")]
pub(crate) struct JsonEncoder {
    indent: Option<usize>,
    sort_keys: bool,
}

#[pymethods]
impl JsonEncoder {
    #[new]
    #[pyo3(signature = (*, indent = None, sort_keys = false))]
    fn new(indent: Option<usize>, sort_keys: bool) -> Self {
        Self { indent, sort_keys }
    }

    fn encode<'py>(&self, obj: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyBytes>> {
        encode(obj, self.indent, self.sort_keys)
    }
}

fn encode<'py>(
    obj: &Bound<'py, PyAny>,
    indent: Option<usize>,
    sort_keys: bool,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut writer = JsonWriter::new(indent);
    ObjectWalker::new(&mut writer, sort_keys)
        .value(obj)
        .map_err(|failure| failure.into_error(obj.py()))?;
    Ok(PyBytes::new(obj.py(), &writer.into_bytes()))
}
