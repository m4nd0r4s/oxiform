//! `ox.json.decode` and `ox.json.Decoder`.

use std::borrow::Cow;
use std::sync::Arc;

use oxiform_core::executor::{Failure, execute};
use oxiform_core::issues::{ContextValue, ErrorCode, Issue, Report, ReportKind};
use oxiform_core::options::Options;
use oxiform_formats::json::JsonReader;
use pyo3::buffer::PyBuffer;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use crate::compiled::{CompiledType, compiled_for};
use crate::errors::report_error;
use crate::options::{PyLimits, decode_options, max_errors_argument};
use crate::python_objects::any_type;
use crate::sink::PythonSink;

/// Reads one JSON document as `type` (any type when omitted), validating it in the same pass.
///
/// `unknown` (`"ignore"` or `"forbid"`) overrides, for this call, what every model does with keys
/// that match no field.
#[pyfunction]
#[pyo3(signature = (data, r#type = None, *, mode = "strict", limits = None, max_errors = 100, unknown = None))]
pub(crate) fn json_decode<'py>(
    data: &Bound<'py, PyAny>,
    r#type: Option<&Bound<'py, PyAny>>,
    mode: &str,
    limits: Option<&PyLimits>,
    #[pyo3(from_py_with = max_errors_argument)] max_errors: usize,
    unknown: Option<&str>,
) -> PyResult<Bound<'py, PyAny>> {
    let target = match r#type {
        Some(target) => target.clone(),
        None => any_type(data.py())?.clone(),
    };
    let compiled = compiled_for(&target)?;
    let options = decode_options(mode, limits, max_errors, unknown)?;
    decode_with(data, &target, &compiled, &options)
}

/// A reusable JSON decoder for one type: `ox.json.Decoder(User).decode(data)`.
#[pyclass(frozen, name = "JsonDecoder", module = "oxiform._oxiform")]
pub(crate) struct JsonDecoder {
    target: Py<PyAny>,
    compiled: Arc<CompiledType>,
    options: Options,
}

#[pymethods]
impl JsonDecoder {
    #[new]
    #[pyo3(signature = (r#type = None, *, mode = "strict", limits = None, max_errors = 100, unknown = None))]
    fn new(
        py: Python<'_>,
        r#type: Option<&Bound<'_, PyAny>>,
        mode: &str,
        limits: Option<&PyLimits>,
        #[pyo3(from_py_with = max_errors_argument)] max_errors: usize,
        unknown: Option<&str>,
    ) -> PyResult<Self> {
        let target = match r#type {
            Some(target) => target.clone(),
            None => any_type(py)?.clone(),
        };
        let compiled = compiled_for(&target)?;
        Ok(Self {
            target: target.unbind(),
            compiled,
            options: decode_options(mode, limits, max_errors, unknown)?,
        })
    }

    fn decode<'py>(&self, data: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        decode_with(
            data,
            self.target.bind(data.py()),
            &self.compiled,
            &self.options,
        )
    }

    #[getter(r#type)]
    fn target<'py>(&self, py: Python<'py>) -> Bound<'py, PyAny> {
        self.target.bind(py).clone()
    }
}

fn decode_with<'py>(
    data: &Bound<'py, PyAny>,
    target: &Bound<'py, PyAny>,
    compiled: &CompiledType,
    options: &Options,
) -> PyResult<Bound<'py, PyAny>> {
    let py = data.py();
    let input = read_input(data)?;
    match decode_bytes(py, &input, compiled, options) {
        Ok(value) => Ok(value),
        Err(Failure::Invalid(report)) => Err(report_error(py, &report, target, "json")),
        Err(Failure::Sink(error)) => Err(error),
    }
}

/// Reads the JSON document in `input` as the type of `compiled`: the core of `decode`, which the
/// embedding ABI calls with a buffer of its host.
pub(crate) fn decode_bytes<'py>(
    py: Python<'py>,
    input: &[u8],
    compiled: &CompiledType,
    options: &Options,
) -> Result<Bound<'py, PyAny>, Failure<PyErr>> {
    let limits = &options.limits;
    if input.len() > limits.max_bytes {
        let issue = Issue {
            code: ErrorCode::LimitExceeded,
            path: Vec::new(),
            message: format!(
                "the document exceeds the limit max_bytes={}",
                limits.max_bytes
            ),
            input: None,
            context: vec![("limit", ContextValue::Text("max_bytes".into()))],
        };
        return Err(Failure::Invalid(decode_report(issue)));
    }
    let text = match std::str::from_utf8(input) {
        Ok(text) => text,
        Err(error) => {
            return Err(Failure::Invalid(decode_report(invalid_utf8(input, error))));
        }
    };
    let mut reader = JsonReader::new(text, limits.max_depth).with_max_str_len(limits.max_str_len);
    let mut sink = PythonSink::new(py, compiled);
    execute(&compiled.plan, &mut reader, &mut sink, options)
}

/// The bytes of the input. `bytes` and `str` are borrowed; anything else that exposes a
/// buffer is copied first, since Python code may change it while it is read.
pub(crate) fn read_input<'data>(data: &'data Bound<'_, PyAny>) -> PyResult<Cow<'data, [u8]>> {
    if let Ok(bytes) = data.cast::<PyBytes>() {
        return Ok(Cow::Borrowed(bytes.as_bytes()));
    }
    if let Ok(text) = data.cast::<PyString>() {
        return Ok(Cow::Borrowed(text.to_str()?.as_bytes()));
    }
    let buffer = PyBuffer::<u8>::get(data)?;
    Ok(Cow::Owned(buffer.to_vec(data.py())?))
}

fn decode_report(issue: Issue) -> Report {
    Report {
        kind: ReportKind::Decode,
        issues: vec![issue],
        truncated: false,
    }
}

fn invalid_utf8(input: &[u8], error: std::str::Utf8Error) -> Issue {
    let offset = error.valid_up_to();
    let valid = std::str::from_utf8(&input[..offset]).unwrap_or_default();
    let line = valid.matches('\n').count() + 1;
    let column = valid
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    Issue {
        code: ErrorCode::Syntax,
        path: Vec::new(),
        message: format!("invalid UTF-8 at byte {offset} (line {line}, column {column})"),
        input: None,
        context: vec![
            (
                "line",
                ContextValue::Integer(i64::try_from(line).unwrap_or(i64::MAX)),
            ),
            (
                "column",
                ContextValue::Integer(i64::try_from(column).unwrap_or(i64::MAX)),
            ),
        ],
    }
}
