//! `ox.urlencoded.decode` and `ox.pairs.decode`: a model read from the pairs of a query string, a
//! form, headers or cookies, in lax mode.

use oxiform_core::executor::{Failure, execute_pairs};
use oxiform_core::options::Options;
use oxiform_core::plan::Node;
use oxiform_formats::pairs::PairsBuilder;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use crate::compiled::{CompiledType, compiled_for};
use crate::decode::read_input;
use crate::errors::report_error;
use crate::options::{PyLimits, context_argument, decode_options, max_errors_argument};
use crate::sink::PythonSink;

/// What pairs are read from.
pub(crate) enum PairsSource<'a> {
    /// A query string or a form body.
    Urlencoded(&'a [u8]),
    /// Name/value pairs as they are.
    Pairs(&'a [(&'a [u8], &'a [u8])]),
}

/// Whether `compiled` is the plan of a model, which is all pairs can be read as. The message says
/// what to declare.
pub(crate) fn require_model(compiled: &CompiledType) -> Result<(), &'static str> {
    let plan = &compiled.plan;
    if matches!(plan.node(plan.root()), Node::Struct(_)) {
        Ok(())
    } else {
        Err(
            "pairs are read as a model: declare the parameters as a class (@ox.model, a dataclass, \
             a TypedDict...) with one field per name, and pass that class",
        )
    }
}

/// Reads `source` as the model of `compiled` in lax mode: the core of `ox.urlencoded.decode`,
/// `ox.pairs.decode` and of the embedding ABI's `decode_pairs`.
pub(crate) fn decode_pairs<'py>(
    py: Python<'py>,
    source: PairsSource<'_>,
    compiled: &CompiledType,
    options: &Options,
    case_insensitive: bool,
    context: Option<Bound<'py, PyAny>>,
) -> Result<Bound<'py, PyAny>, Failure<PyErr>> {
    let mut builder = PairsBuilder::new(&options.limits, case_insensitive);
    match source {
        PairsSource::Urlencoded(input) => builder.add_urlencoded(input),
        PairsSource::Pairs(pairs) => {
            for (name, value) in pairs {
                builder.add_pair(name, value);
            }
        }
    }
    let mut reader = builder.finish();
    let mut sink = PythonSink::new(py, compiled, context);
    execute_pairs(
        &compiled.plan,
        &mut reader,
        &mut sink,
        options,
        case_insensitive,
    )
}

fn model_of<'py>(
    function: &str,
    target: &Bound<'py, PyAny>,
) -> PyResult<std::sync::Arc<CompiledType>> {
    let compiled = compiled_for(target)?;
    require_model(&compiled).map_err(|hint| {
        PyTypeError::new_err(format!(
            "{function} cannot read pairs as {}: {hint}",
            target
                .repr()
                .map(|text| text.to_string())
                .unwrap_or_default()
        ))
    })?;
    Ok(compiled)
}

/// Reads a query string or an `application/x-www-form-urlencoded` body (`bytes` or `str`) as a
/// model, in lax mode: `%XX` and `+` are decoded, a name that is repeated gives a list, a tuple or
/// a set its values in order, and issues name the field by its wire name (`/page`).
#[pyfunction]
#[pyo3(name = "urlencoded_decode", signature = (data, r#type, *, context = None, limits = None, max_errors = 100, unknown = None))]
pub(crate) fn urlencoded_decode<'py>(
    data: &Bound<'py, PyAny>,
    r#type: &Bound<'py, PyAny>,
    context: Option<&Bound<'py, PyAny>>,
    limits: Option<&PyLimits>,
    #[pyo3(from_py_with = max_errors_argument)] max_errors: usize,
    unknown: Option<&str>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = data.py();
    let compiled = model_of("ox.urlencoded.decode", r#type)?;
    let options = decode_options("lax", limits, max_errors, unknown)?;
    let input = read_input(data)?;
    finish(
        py,
        decode_pairs(
            py,
            PairsSource::Urlencoded(&input),
            &compiled,
            &options,
            false,
            context_argument(context),
        ),
        r#type,
        "urlencoded",
    )
}

/// Reads name/value pairs (headers, cookies) as a model, in lax mode, without decoding anything:
/// names and values are `str` or UTF-8 `bytes`. `case_insensitive=True` matches names without
/// regard to ASCII case, as header names are matched; a name that is repeated gives a list, a
/// tuple or a set its values in order.
#[pyfunction]
#[pyo3(name = "pairs_decode", signature = (pairs, r#type, *, case_insensitive = false, context = None, limits = None, max_errors = 100, unknown = None))]
pub(crate) fn pairs_decode<'py>(
    pairs: &Bound<'py, PyAny>,
    r#type: &Bound<'py, PyAny>,
    case_insensitive: bool,
    context: Option<&Bound<'py, PyAny>>,
    limits: Option<&PyLimits>,
    #[pyo3(from_py_with = max_errors_argument)] max_errors: usize,
    unknown: Option<&str>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = pairs.py();
    let compiled = model_of("ox.pairs.decode", r#type)?;
    let options = decode_options("lax", limits, max_errors, unknown)?;
    // Everything is read out of the iterable first, so no Python code runs while decoding but the
    // sink's, and the texts stay alive (and unchanged: `str` and `bytes` are immutable) throughout.
    let texts = collect_pairs(pairs)?;
    let views: Vec<(&[u8], &[u8])> = texts
        .iter()
        .map(|(name, value)| Ok((name.bytes()?, value.bytes()?)))
        .collect::<PyResult<_>>()?;
    finish(
        py,
        decode_pairs(
            py,
            PairsSource::Pairs(&views),
            &compiled,
            &options,
            case_insensitive,
            context_argument(context),
        ),
        r#type,
        "pairs",
    )
}

/// A `str` or `bytes` that holds the text of a name or value.
enum Text<'py> {
    Str(Bound<'py, PyString>),
    Bytes(Bound<'py, PyBytes>),
}

impl Text<'_> {
    fn bytes(&self) -> PyResult<&[u8]> {
        match self {
            Text::Str(text) => Ok(text.to_str()?.as_bytes()),
            Text::Bytes(bytes) => Ok(bytes.as_bytes()),
        }
    }
}

fn collect_pairs<'py>(pairs: &Bound<'py, PyAny>) -> PyResult<Vec<(Text<'py>, Text<'py>)>> {
    let wrong = |what: String| {
        PyTypeError::new_err(format!(
            "ox.pairs.decode takes an iterable of (name, value) pairs of str or bytes, such as \
             headers.items() or [(\"page\", \"2\")]; {what}"
        ))
    };
    // A mapping gives its items; anything else is read as the pairs themselves.
    let source = if pairs.hasattr("items")? && !pairs.is_instance_of::<pyo3::types::PyList>() {
        pairs.call_method0("items")?
    } else {
        pairs.clone()
    };
    let text = |value: Bound<'py, PyAny>, role: &str| -> PyResult<Text<'py>> {
        if let Ok(text) = value.clone().cast_into::<PyString>() {
            Ok(Text::Str(text))
        } else if let Ok(bytes) = value.clone().cast_into::<PyBytes>() {
            Ok(Text::Bytes(bytes))
        } else {
            Err(wrong(format!("a {role} is {}", value.get_type().name()?)))
        }
    };
    let mut collected = Vec::new();
    for item in source.try_iter().map_err(|_| {
        wrong(format!(
            "got {}",
            pairs
                .get_type()
                .name()
                .map(|n| n.to_string())
                .unwrap_or_default()
        ))
    })? {
        let item = item?;
        let (name, value): (Bound<'py, PyAny>, Bound<'py, PyAny>) =
            item.extract().map_err(|_| {
                wrong(format!(
                    "an item is {}, not a pair",
                    item.get_type()
                        .name()
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                ))
            })?;
        collected.push((text(name, "name")?, text(value, "value")?));
    }
    Ok(collected)
}

fn finish<'py>(
    py: Python<'py>,
    outcome: Result<Bound<'py, PyAny>, Failure<PyErr>>,
    target: &Bound<'py, PyAny>,
    format: &str,
) -> PyResult<Bound<'py, PyAny>> {
    match outcome {
        Ok(value) => Ok(value),
        Err(Failure::Invalid(report)) => Err(report_error(py, &report, target, format)),
        Err(Failure::Sink(error)) => Err(error),
    }
}
