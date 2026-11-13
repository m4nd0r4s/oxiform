//! `ox.json.encode` and `ox.json.Encoder`.

use std::sync::Arc;

use oxiform_formats::json::JsonWriter;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use super::ObjectWalker;
use super::declared::{DeclaredPlan, declared_for};

/// Writes `obj` as JSON: compact, or indented by `indent` spaces; keys sorted with `sort_keys`.
///
/// With `type`, `obj` is written as that type: the fields of `type` are written, whatever the
/// class of `obj` has besides.
#[pyfunction]
#[pyo3(signature = (obj, *, r#type = None, indent = None, sort_keys = false))]
pub(crate) fn json_encode<'py>(
    obj: &Bound<'py, PyAny>,
    r#type: Option<&Bound<'py, PyAny>>,
    indent: Option<usize>,
    sort_keys: bool,
) -> PyResult<Bound<'py, PyBytes>> {
    let plan = r#type.map(declared_for).transpose()?;
    encode(obj, plan.as_deref(), indent, sort_keys)
}

/// A reusable JSON encoder with fixed options.
#[pyclass(frozen, name = "JsonEncoder", module = "oxiform._oxiform")]
pub(crate) struct JsonEncoder {
    /// The type written when `encode` is not given one, and its plan.
    default: Option<(Py<PyAny>, Arc<DeclaredPlan>)>,
    indent: Option<usize>,
    sort_keys: bool,
}

#[pymethods]
impl JsonEncoder {
    #[new]
    #[pyo3(signature = (*, r#type = None, indent = None, sort_keys = false))]
    fn new(
        r#type: Option<&Bound<'_, PyAny>>,
        indent: Option<usize>,
        sort_keys: bool,
    ) -> PyResult<Self> {
        let default = r#type
            .map(|target| declared_for(target).map(|plan| (target.clone().unbind(), plan)))
            .transpose()?;
        Ok(Self {
            default,
            indent,
            sort_keys,
        })
    }

    #[pyo3(signature = (obj, *, r#type = None))]
    fn encode<'py>(
        &self,
        obj: &Bound<'py, PyAny>,
        r#type: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        match r#type {
            Some(target) => {
                let plan = declared_for(target)?;
                encode(obj, Some(&plan), self.indent, self.sort_keys)
            }
            None => encode(
                obj,
                self.default.as_ref().map(|(_, plan)| plan.as_ref()),
                self.indent,
                self.sort_keys,
            ),
        }
    }

    /// The type written when `encode` is not given one, or `None`.
    #[getter(r#type)]
    fn target<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyAny>> {
        self.default
            .as_ref()
            .map(|(target, _)| target.bind(py).clone())
    }
}

fn encode<'py>(
    obj: &Bound<'py, PyAny>,
    plan: Option<&DeclaredPlan>,
    indent: Option<usize>,
    sort_keys: bool,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut writer = JsonWriter::new(indent);
    let mut walker = ObjectWalker::new(&mut writer, sort_keys);
    match plan {
        Some(plan) => walker.declared_root(plan, obj),
        None => walker.value(obj),
    }
    .map_err(|failure| failure.into_error(obj.py()))?;
    Ok(PyBytes::new(obj.py(), &writer.into_bytes()))
}
