//! Turns a report into `ox.ValidationError` or `ox.DecodeError`.

use oxiform_core::description::ChoiceValue;
use oxiform_core::issues::{ContextValue, ErrorCode, Issue, PathSegment, Report, ReportKind};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::python_objects::{decode_error, validation_error};

/// The exception for `report`, raised while reading `format` as `target`.
pub(crate) fn report_error(
    py: Python<'_>,
    report: &Report,
    target: &Bound<'_, PyAny>,
    format: &str,
) -> PyErr {
    match build_error(py, report, target, format) {
        Ok(error) => error,
        Err(error) => error,
    }
}

fn build_error(
    py: Python<'_>,
    report: &Report,
    target: &Bound<'_, PyAny>,
    format: &str,
) -> PyResult<PyErr> {
    let class = match report.kind {
        ReportKind::Validation => validation_error(py)?,
        ReportKind::Decode => decode_error(py)?,
    };
    let errors = PyList::empty(py);
    for issue in &report.issues {
        errors.append(issue_dict(py, issue, format)?)?;
    }
    let keywords = PyDict::new(py);
    keywords.set_item("truncated", report.truncated)?;
    keywords.set_item("target", target_name(target))?;
    keywords.set_item("format", format)?;
    let exception = class.call((errors,), Some(&keywords))?;
    Ok(PyErr::from_value(exception))
}

fn issue_dict<'py>(py: Python<'py>, issue: &Issue, format: &str) -> PyResult<Bound<'py, PyDict>> {
    let path = PyList::empty(py);
    for segment in &issue.path {
        match segment {
            PathSegment::Key(key) => path.append(key)?,
            PathSegment::Index(index) => path.append(index)?,
        }
    }
    let context = PyDict::new(py);
    for (key, value) in &issue.context {
        match value {
            ContextValue::Bool(value) => context.set_item(key, value)?,
            ContextValue::Integer(value) => context.set_item(key, value)?,
            ContextValue::Float(value) => context.set_item(key, value)?,
            ContextValue::Text(value) => context.set_item(key, value)?,
            ContextValue::Integers(values) => context.set_item(key, values)?,
            ContextValue::Texts(values) => context.set_item(key, values)?,
            ContextValue::Values(values) => {
                let list = PyList::empty(py);
                for value in values {
                    match value {
                        ChoiceValue::Null => list.append(py.None())?,
                        ChoiceValue::Bool(flag) => list.append(flag)?,
                        ChoiceValue::Integer(number) => list.append(number)?,
                        ChoiceValue::String(text) => list.append(text)?,
                    }
                }
                context.set_item(key, list)?;
            }
            ContextValue::Variants(variants) => {
                let by_variant = PyDict::new(py);
                for (name, issues) in variants {
                    let errors = PyList::empty(py);
                    for issue in issues {
                        errors.append(issue_dict(py, issue, format)?)?;
                    }
                    by_variant.set_item(name, errors)?;
                }
                context.set_item(key, by_variant)?;
            }
        }
    }
    if issue.code == ErrorCode::Syntax {
        context.set_item("format", format)?;
    }
    let dict = PyDict::new(py);
    dict.set_item("code", issue.code.as_str())?;
    dict.set_item("path", path)?;
    dict.set_item("pointer", issue.pointer())?;
    dict.set_item("message", &issue.message)?;
    if let Some(input) = &issue.input {
        dict.set_item("input", input)?;
    }
    dict.set_item("ctx", context)?;
    Ok(dict)
}

/// The qualified name of a class, or the repr of any other type expression.
fn target_name(target: &Bound<'_, PyAny>) -> String {
    let is_class = target.is_instance_of::<pyo3::types::PyType>();
    let name = if is_class {
        target.getattr("__qualname__").ok()
    } else {
        None
    };
    match name.and_then(|name| name.extract::<String>().ok()) {
        Some(name) => name,
        None => target
            .repr()
            .map(|repr| repr.to_string())
            .unwrap_or_else(|_| "?".into()),
    }
}
