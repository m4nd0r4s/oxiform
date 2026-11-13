//! The functions of the table that run with the interpreter: decode and encode, and what they
//! share (the attached guard and the mapping of a status to a Python exception).

use oxiform_abi::{
    OX_BUFFER, OX_FORMAT_JSON, OX_FORMAT_PAIRS, OX_FORMAT_URLENCODED, OX_INTERNAL, OX_INVALID,
    OX_INVALID_ARGUMENT, OX_OK, OX_PYTHON_ERROR, OX_UNSUPPORTED, OxBuffer, OxDecodeOptions,
    OxEncodeOptions, OxPair, OxPairsOptions, OxPlan, OxPyObject, OxReport,
};
use oxiform_abi_export::{
    Error, append, check_buffer, decode_options, encode_options, into_report_pointer, pairs_options,
};
use oxiform_core::executor::Failure;
use pyo3::exceptions::{PyBufferError, PyNotImplementedError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use super::plan::handle;
use crate::decode::decode_bytes;
use crate::encode::json::encode_to_vec;
use crate::pairs::{PairsSource, decode_pairs as decode_pairs_source, require_model};

/// Runs `call` with the thread attached, and turns a panic into `OX_INTERNAL` with a
/// `RuntimeError` set, so nothing unwinds across the boundary.
pub(super) fn attached(call: impl FnOnce(Python<'_>) -> i32) -> i32 {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Python::attach(call)));
    outcome.unwrap_or_else(|payload| {
        let detail = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Python::attach(|py| {
            PyRuntimeError::new_err(format!(
                "oxiform panicked inside an embedding ABI call ({detail}); this is a bug in \
                 oxiform, please report it with the call that caused it"
            ))
            .restore(py);
        });
        OX_INTERNAL
    })
}

/// Sets `error` as the exception of the thread and returns `status`.
pub(super) fn raise(py: Python<'_>, status: i32, error: PyErr) -> i32 {
    error.restore(py);
    status
}

/// An `OX_INVALID_ARGUMENT` with a `ValueError` that says what to pass.
pub(super) fn invalid_argument(py: Python<'_>, message: &str) -> i32 {
    raise(
        py,
        OX_INVALID_ARGUMENT,
        PyValueError::new_err(message.to_owned()),
    )
}

/// An engine-side error as its status and a Python exception.
fn raise_error(py: Python<'_>, error: Error) -> i32 {
    let exception = match error.status {
        OX_UNSUPPORTED => PyNotImplementedError::new_err(error.message),
        OX_BUFFER => PyBufferError::new_err(error.message),
        OX_INTERNAL => PyRuntimeError::new_err(error.message),
        _ => PyValueError::new_err(error.message),
    };
    raise(py, error.status, exception)
}

fn json_only(py: Python<'_>, format: u32) -> Option<i32> {
    (format != OX_FORMAT_JSON).then(|| {
        raise_error(
            py,
            Error::new(
                OX_UNSUPPORTED,
                format!(
                    "format {format} is not supported by this oxiform; ABI 1.0 reads and writes \
                     OX_FORMAT_JSON (1) only, and later formats arrive in later minor versions: \
                     upgrade oxiform"
                ),
            ),
        )
    })
}

/// # Safety
///
/// See [`oxiform_abi::OxiformApi::decode`].
pub(super) unsafe extern "C" fn decode(
    plan: *const OxPlan,
    format: u32,
    data: *const u8,
    len: usize,
    options: *const OxDecodeOptions,
    out_value: *mut *mut OxPyObject,
    out_report: *mut *mut OxReport,
) -> i32 {
    attached(|py| {
        if out_value.is_null() || out_report.is_null() {
            return invalid_argument(
                py,
                "decode was given a NULL out_value or out_report; pass the addresses of two pointers",
            );
        }
        // SAFETY: both are non-null and the caller guarantees they are writable.
        unsafe {
            out_value.write(core::ptr::null_mut());
            out_report.write(core::ptr::null_mut());
        }
        if let Some(status) = json_only(py, format) {
            return status;
        }
        // SAFETY: the caller guarantees `plan` is live for the call.
        let Ok(plan) = (unsafe { handle(plan) }) else {
            return invalid_argument(py, "decode was given a NULL plan; get one with plan_for");
        };
        if data.is_null() && len > 0 {
            return invalid_argument(
                py,
                "decode was given a NULL data pointer with a length above 0; pass the document bytes",
            );
        }
        // SAFETY: the caller guarantees `options` is null or readable as its `struct_size` says.
        let options = match unsafe { decode_options(options) } {
            Ok(options) => options,
            Err(error) => return raise_error(py, error),
        };
        let input: &[u8] = if len == 0 {
            &[]
        } else {
            // SAFETY: the caller guarantees `len` bytes at `data` are readable and do not change
            // during the call.
            unsafe { core::slice::from_raw_parts(data, len) }
        };
        match decode_bytes(py, input, &plan.compiled, &options) {
            Ok(value) => {
                // SAFETY: `out_value` is writable; `into_ptr` gives the reference the host owns.
                unsafe { out_value.write(value.into_ptr().cast::<OxPyObject>()) };
                OX_OK
            }
            Err(Failure::Invalid(report)) => {
                // SAFETY: `out_report` is writable.
                unsafe { out_report.write(into_report_pointer(report, "json")) };
                OX_INVALID
            }
            Err(Failure::Sink(error)) => raise(py, OX_PYTHON_ERROR, error),
        }
    })
}

/// # Safety
///
/// See [`oxiform_abi::OxiformApi::encode`].
pub(super) unsafe extern "C" fn encode(
    format: u32,
    obj: *mut OxPyObject,
    plan: *const OxPlan,
    options: *const OxEncodeOptions,
    buffer: *mut OxBuffer,
) -> i32 {
    attached(|py| {
        if let Some(status) = json_only(py, format) {
            return status;
        }
        if obj.is_null() {
            return invalid_argument(
                py,
                "encode was given a NULL object; pass a borrowed PyObject* to write",
            );
        }
        // SAFETY: the caller guarantees `options` is null or readable as its `struct_size` says.
        let params = match unsafe { encode_options(options) } {
            Ok(params) => params,
            Err(error) => return raise_error(py, error),
        };
        // A bad buffer is refused before any work is done.
        // SAFETY: the caller guarantees `buffer` is null or a valid host buffer.
        if let Err(error) = unsafe { check_buffer(buffer) } {
            return raise_error(py, error);
        }
        let declared = if plan.is_null() {
            None
        } else {
            // SAFETY: the caller guarantees a non-null `plan` is live for the call.
            let Ok(plan) = (unsafe { handle(plan) }) else {
                return invalid_argument(
                    py,
                    "encode was given an unusable plan; get one with plan_for",
                );
            };
            match plan.declared(py) {
                Ok(declared) => Some(declared),
                Err(error) => return raise(py, OX_PYTHON_ERROR, error),
            }
        };
        // SAFETY: the caller guarantees `obj` is a valid `PyObject*` alive during the call, and
        // the thread is attached.
        let obj = unsafe { Bound::from_borrowed_ptr(py, obj.cast()) };
        let bytes = match encode_to_vec(&obj, declared.as_deref(), params.indent, params.sort_keys)
        {
            Ok(bytes) => bytes,
            Err(error) => return raise(py, OX_PYTHON_ERROR, error),
        };
        // SAFETY: the caller guarantees `buffer` is a valid host buffer.
        match unsafe { append(buffer, &bytes) } {
            Ok(()) => OX_OK,
            Err(error) => raise_error(py, error),
        }
    })
}

/// # Safety
///
/// See [`oxiform_abi::OxiformApi::decode_pairs`].
pub(super) unsafe extern "C" fn decode_pairs(
    plan: *const OxPlan,
    format: u32,
    data: *const u8,
    len: usize,
    pairs: *const OxPair,
    pair_count: usize,
    options: *const OxPairsOptions,
    out_value: *mut *mut OxPyObject,
    out_report: *mut *mut OxReport,
) -> i32 {
    attached(|py| {
        if out_value.is_null() || out_report.is_null() {
            return invalid_argument(
                py,
                "decode_pairs was given a NULL out_value or out_report; pass the addresses of two pointers",
            );
        }
        // SAFETY: both are non-null and the caller guarantees they are writable.
        unsafe {
            out_value.write(core::ptr::null_mut());
            out_report.write(core::ptr::null_mut());
        }
        if format != OX_FORMAT_URLENCODED && format != OX_FORMAT_PAIRS {
            return raise_error(
                py,
                Error::new(
                    OX_UNSUPPORTED,
                    format!(
                        "format {format} is not a source of pairs; decode_pairs reads \
                         OX_FORMAT_URLENCODED (2) and OX_FORMAT_PAIRS (3), and decode reads \
                         documents: upgrade oxiform if you were built for a later minor version"
                    ),
                ),
            );
        }
        // SAFETY: the caller guarantees `plan` is live for the call.
        let Ok(plan) = (unsafe { handle(plan) }) else {
            return invalid_argument(
                py,
                "decode_pairs was given a NULL plan; get one with plan_for",
            );
        };
        if let Err(hint) = require_model(&plan.compiled) {
            return invalid_argument(
                py,
                &format!("decode_pairs was given the plan of a type that is not a model: {hint}"),
            );
        }
        let urlencoded = format == OX_FORMAT_URLENCODED;
        let (unused, unused_what, used) = if urlencoded {
            (
                !pairs.is_null() || pair_count != 0,
                "pairs and pair_count",
                "data",
            )
        } else {
            (!data.is_null() || len != 0, "data and len", "pairs")
        };
        if unused {
            return invalid_argument(
                py,
                &format!(
                    "decode_pairs was given {unused_what} for a source that reads {used}; pass \
                     NULL and 0 for the one it does not read"
                ),
            );
        }
        if (urlencoded && data.is_null() && len > 0)
            || (!urlencoded && pairs.is_null() && pair_count > 0)
        {
            return invalid_argument(
                py,
                &format!(
                    "decode_pairs was given a NULL {used} pointer with a length above 0; pass the source"
                ),
            );
        }
        // SAFETY: the caller guarantees `options` is null or readable as its `struct_size` says.
        let params = match unsafe { pairs_options(options) } {
            Ok(params) => params,
            Err(error) => return raise_error(py, error),
        };
        let outcome = if urlencoded {
            let input: &[u8] = if len == 0 {
                &[]
            } else {
                // SAFETY: `len` bytes at `data` are readable and unchanged during the call.
                unsafe { core::slice::from_raw_parts(data, len) }
            };
            decode_pairs_source(
                py,
                PairsSource::Urlencoded(input),
                &plan.compiled,
                &params.options,
                params.case_insensitive,
            )
        } else {
            let raw: &[OxPair] = if pair_count == 0 {
                &[]
            } else {
                // SAFETY: `pair_count` pairs at `pairs` are readable and unchanged during the call.
                unsafe { core::slice::from_raw_parts(pairs, pair_count) }
            };
            let mut views = Vec::with_capacity(raw.len());
            for pair in raw {
                let (Some(name), Some(value)) = (
                    // SAFETY: each string of a pair is `len` readable bytes (or null with 0).
                    unsafe { bytes_of(pair.name.ptr, pair.name.len) },
                    unsafe { bytes_of(pair.value.ptr, pair.value.len) },
                ) else {
                    return invalid_argument(
                        py,
                        "decode_pairs was given a pair with a NULL string and a length above 0; \
                         pass the bytes of every name and value",
                    );
                };
                views.push((name, value));
            }
            decode_pairs_source(
                py,
                PairsSource::Pairs(&views),
                &plan.compiled,
                &params.options,
                params.case_insensitive,
            )
        };
        match outcome {
            Ok(value) => {
                // SAFETY: `out_value` is writable; `into_ptr` gives the reference the host owns.
                unsafe { out_value.write(value.into_ptr().cast::<OxPyObject>()) };
                OX_OK
            }
            Err(Failure::Invalid(report)) => {
                let format = if urlencoded { "urlencoded" } else { "pairs" };
                // SAFETY: `out_report` is writable.
                unsafe { out_report.write(into_report_pointer(report, format)) };
                OX_INVALID
            }
            Err(Failure::Sink(error)) => raise(py, OX_PYTHON_ERROR, error),
        }
    })
}

/// The bytes at `pointer`, empty for length 0, or `None` for a null pointer with a length.
///
/// # Safety
///
/// A non-null `pointer` must be readable for `len` bytes for as long as `'a`.
unsafe fn bytes_of<'a>(pointer: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        Some(&[])
    } else if pointer.is_null() {
        None
    } else {
        // SAFETY: forwarded from the caller.
        Some(unsafe { core::slice::from_raw_parts(pointer, len) })
    }
}
