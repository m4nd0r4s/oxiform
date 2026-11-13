//! The host's own RFC 9457 problem, built from the issues oxiform returns.

use oxiform_abi::{ChoiceValue, ContextValue, IssueView, Report, ReportView, Segment};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

pyo3::create_exception!(
    oxiform_abi_host,
    HostProblem,
    PyValueError,
    "Invalid input. `args[0]` is the problem the host built from the issues of oxiform."
);

pub fn problem_of(py: Python<'_>, report: &Report) -> PyResult<Py<PyDict>> {
    problem_from_view(py, &report.view())
}

/// The problem of a report: the status is the host's decision (400 for input that could not be read,
/// 413 for a limit, 422 for input that does not fit the type), and the issues are those of
/// `ox.ValidationError.errors()`, field by field.
pub fn problem_from_view(py: Python<'_>, view: &ReportView<'_>) -> PyResult<Py<PyDict>> {
    let errors = PyList::empty(py);
    let mut limit = false;
    for issue in view.issues() {
        limit |= issue.code() == "limit_exceeded";
        errors.append(issue_dict(py, &issue)?)?;
    }
    let status = match (view.is_decode_error(), limit) {
        (true, true) => 413,
        (true, false) => 400,
        (false, _) => 422,
    };
    let title = match status {
        400 => "Bad Request",
        413 => "Content Too Large",
        _ => "Unprocessable Content",
    };
    let problem = PyDict::new(py);
    problem.set_item("type", "about:blank")?;
    problem.set_item("title", title)?;
    problem.set_item("status", status)?;
    problem.set_item("detail", format!("{} error(s)", view.len()))?;
    problem.set_item("truncated", view.truncated())?;
    problem.set_item("errors", errors)?;
    Ok(problem.unbind())
}

fn issue_dict<'py>(py: Python<'py>, issue: &IssueView<'_>) -> PyResult<Bound<'py, PyDict>> {
    let path = PyList::empty(py);
    for segment in issue.path() {
        match segment {
            Segment::Key(key) => path.append(key)?,
            Segment::Index(index) => path.append(index)?,
        }
    }
    let context = PyDict::new(py);
    for (key, value) in issue.context() {
        context.set_item(key, context_value(py, &value)?)?;
    }
    let dict = PyDict::new(py);
    dict.set_item("code", issue.code())?;
    dict.set_item("path", path)?;
    dict.set_item("pointer", issue.pointer())?;
    dict.set_item("message", issue.message())?;
    if let Some(input) = issue.input() {
        dict.set_item("input", input)?;
    }
    dict.set_item("ctx", context)?;
    Ok(dict)
}

fn context_value<'py>(py: Python<'py>, value: &ContextValue<'_>) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        ContextValue::Bool(flag) => flag.into_pyobject(py)?.to_owned().into_any(),
        ContextValue::Integer(number) => number.into_pyobject(py)?.into_any(),
        ContextValue::Float(number) => number.into_pyobject(py)?.into_any(),
        ContextValue::Text(text) => text.into_pyobject(py)?.into_any(),
        ContextValue::Integers(numbers) => PyList::new(py, numbers.iter())?.into_any(),
        ContextValue::Texts(texts) => PyList::new(py, texts.iter())?.into_any(),
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
            list.into_any()
        }
        ContextValue::Variants(variants) => {
            let by_variant = PyDict::new(py);
            for (name, issues) in variants {
                let list = PyList::empty(py);
                for issue in issues {
                    list.append(issue_dict(py, issue)?)?;
                }
                by_variant.set_item(name, list)?;
            }
            by_variant.into_any()
        }
    })
}
