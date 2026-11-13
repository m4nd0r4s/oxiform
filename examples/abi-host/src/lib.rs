//! A sample host of the oxiform embedding ABI.
//!
//! This is what `douro._native` does with oxiform, in miniature: it depends on `oxiform-abi` (types
//! and the version check) and on PyO3, **not** on oxiform's engine, finds the capsule of the
//! installed `oxiform` when it is imported, and then
//!
//! - asks for plans (`plan_for`), which are the ones of `ox.json.decode`: there is no second
//!   plan cache in this module;
//! - decodes from a byte buffer it owns, with every option, and builds its own RFC 9457 problem
//!   from the structured issues it gets back (`Plan.decode`, `decode_request`);
//! - encodes into a growable buffer it owns, against a declared type (`encode`).
//!
//! The `*_raw` functions bypass the safe layer to let the tests exercise the table as a C
//! host would: odd struct sizes, flags, a context, buffers that fail.

mod buffer;
mod problem;

use std::sync::OnceLock;

use oxiform_abi::{
    Api, DecodeOptions, Decoded, EncodeOptions, Mode, OX_INVALID, OX_OK, OxBuffer, OxDecodeOptions,
    OxEncodeOptions, OxLimits, Report, Status, Unknown, VecBuffer,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple};

use buffer::{BufferMode, HostBuffer};
use problem::{HostProblem, problem_of};

static API: OnceLock<Api> = OnceLock::new();

fn api() -> Api {
    *API.get().expect("the module imported oxiform at init")
}

/// A plan handle: one reference to a compiled type of oxiform, released when it is dropped.
#[pyclass(frozen, module = "oxiform_abi_host")]
struct Plan {
    plan: oxiform_abi::Plan,
}

fn mode_of(mode: &str) -> PyResult<Mode> {
    match mode {
        "strict" => Ok(Mode::Strict),
        "lax" => Ok(Mode::Lax),
        other => Err(PyValueError::new_err(format!(
            "mode must be \"strict\" or \"lax\", not {other:?}"
        ))),
    }
}

fn unknown_of(unknown: &str) -> PyResult<Unknown> {
    match unknown {
        "model" => Ok(Unknown::Model),
        "ignore" => Ok(Unknown::Ignore),
        "forbid" => Ok(Unknown::Forbid),
        other => Err(PyValueError::new_err(format!(
            "unknown must be \"model\", \"ignore\" or \"forbid\", not {other:?}"
        ))),
    }
}

/// `limits` of a call: a dict of the fields to change, over the defaults of the installed oxiform.
fn limits_of(limits: Option<&Bound<'_, PyDict>>) -> PyResult<Option<OxLimits>> {
    let Some(changes) = limits else {
        return Ok(None);
    };
    let mut raw = api().limits_default();
    for (key, value) in changes.iter() {
        let value: u64 = value.extract()?;
        match key.extract::<String>()?.as_str() {
            "max_bytes" => raw.max_bytes = value,
            "max_depth" => raw.max_depth = value,
            "max_items" => raw.max_items = value,
            "max_str_len" => raw.max_str_len = value,
            "max_int_digits" => raw.max_int_digits = value,
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown limit {other:?}; use max_bytes, max_depth, max_items, max_str_len or max_int_digits"
                )));
            }
        }
    }
    Ok(Some(raw))
}

fn bytes_of<'a>(data: &'a Bound<'_, PyAny>) -> PyResult<&'a [u8]> {
    Ok(data.cast::<PyBytes>()?.as_bytes())
}

fn decode_options(
    mode: &str,
    max_errors: u64,
    limits: Option<&Bound<'_, PyDict>>,
    unknown: &str,
) -> PyResult<DecodeOptions> {
    Ok(DecodeOptions {
        mode: mode_of(mode)?,
        max_errors,
        limits: limits_of(limits)?,
        unknown: unknown_of(unknown)?,
    })
}

/// Decodes `data` through the table. An invalid input is the report; a failure that raised a
/// Python exception is that exception.
fn decode_through_table<'py>(
    py: Python<'py>,
    plan: &oxiform_abi::Plan,
    data: &[u8],
    options: &DecodeOptions,
) -> PyResult<Result<Bound<'py, PyAny>, Report>> {
    // SAFETY: this thread is attached (PyO3 gave us `py`), and the value is released through a
    // `Bound`.
    match unsafe { api().decode_json(plan, data, options) } {
        Ok(Decoded::Value(value)) => {
            // SAFETY: the table gave us one new reference, which the `Bound` now owns.
            Ok(Ok(unsafe {
                Bound::from_owned_ptr(py, value.as_ptr().cast())
            }))
        }
        Ok(Decoded::Invalid(report)) => Ok(Err(report)),
        Err(_) => Err(PyErr::fetch(py)),
    }
}

#[pymethods]
impl Plan {
    /// Reads `data` (bytes) as the type of this plan. Invalid input raises `HostProblem`, whose
    /// `problem` is the RFC 9457 object the host built from the issues.
    #[pyo3(signature = (data, *, mode = "strict", max_errors = 0, limits = None, unknown = "model", source = None))]
    #[allow(clippy::too_many_arguments)]
    fn decode<'py>(
        &self,
        py: Python<'py>,
        data: &Bound<'py, PyAny>,
        mode: &str,
        max_errors: u64,
        limits: Option<&Bound<'py, PyDict>>,
        unknown: &str,
        source: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let options = decode_options(mode, max_errors, limits, unknown)?;
        match decode_through_table(py, &self.plan, bytes_of(data)?, &options)? {
            Ok(value) => Ok(value),
            Err(mut report) => {
                if let Some(source) = source {
                    report
                        .in_source(source)
                        .map_err(|status| PyValueError::new_err(api().status_message(status)))?;
                }
                Err(HostProblem::new_err(problem_of(py, &report)?))
            }
        }
    }

    /// `decode` without the safe layer, as a C host calls it: `(status, value, problem, error)`.
    ///
    /// `struct_size` and `extra` change the size the options claim (`extra` bytes of "fields of a
    /// later minor version" follow the struct, set to ones); `flags` and `context` are set as given.
    #[pyo3(signature = (data, *, mode = 0, max_errors = 0, unknown = 0, flags = 0, context = None, struct_size = None, extra = 0, limits_struct_size = None, format = 1, null_data = false))]
    #[allow(clippy::too_many_arguments)]
    fn decode_raw<'py>(
        &self,
        py: Python<'py>,
        data: &Bound<'py, PyAny>,
        mode: u32,
        max_errors: u64,
        unknown: u32,
        flags: u32,
        context: Option<&Bound<'py, PyAny>>,
        struct_size: Option<u32>,
        extra: usize,
        limits_struct_size: Option<u32>,
        format: u32,
        null_data: bool,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let data = bytes_of(data)?;
        let mut limits = api().limits_default();
        if let Some(size) = limits_struct_size {
            limits.struct_size = size;
        }
        let mut raw = OxDecodeOptions::new();
        raw.mode = mode;
        raw.max_errors = max_errors;
        raw.unknown = unknown;
        raw.flags = flags;
        raw.limits = if limits_struct_size.is_some() {
            &raw const limits
        } else {
            core::ptr::null()
        };
        raw.context = context.map_or(core::ptr::null_mut(), |object| object.as_ptr().cast());
        let total = size_of::<OxDecodeOptions>() + extra;
        raw.struct_size = struct_size.unwrap_or(total as u32);
        // Aligned storage for the struct and the bytes that follow it.
        let mut storage = vec![u64::MAX; total.div_ceil(8)];
        let options = storage.as_mut_ptr().cast::<OxDecodeOptions>();
        // SAFETY: `storage` is aligned to 8 and holds `total >= size_of::<OxDecodeOptions>()` bytes.
        unsafe { options.write(raw) };
        let mut value = core::ptr::null_mut();
        let mut report = core::ptr::null_mut();
        // SAFETY: this thread is attached; `plan` is alive; `data`, `options` and `limits` live
        // through the call; the out pointers are valid.
        let status = unsafe {
            (api().table().decode)(
                self.plan.as_ptr(),
                format,
                if null_data {
                    core::ptr::null()
                } else {
                    data.as_ptr()
                },
                data.len(),
                options,
                &raw mut value,
                &raw mut report,
            )
        };
        let error = PyErr::take(py).map(|error| error.into_value(py));
        let value = if value.is_null() {
            None
        } else {
            // SAFETY: a new reference the table gave us.
            Some(unsafe { Bound::from_owned_ptr(py, value.cast()) })
        };
        let problem = if report.is_null() {
            None
        } else {
            // SAFETY: a report the table gave us, which `Report`-like ownership frees; the safe
            // layer's `Report` is built for this through `combine`, so free it through the table.
            let view =
                unsafe { oxiform_abi::ReportView::from_raw(&*(api().table().report_view)(report)) };
            let problem = problem::problem_from_view(py, &view)?;
            // SAFETY: frees the report once.
            unsafe { (api().table().report_free)(report) };
            Some(problem)
        };
        PyTuple::new(
            py,
            [
                status.into_pyobject(py)?.into_any(),
                value.unwrap_or_else(|| py.None().into_bound(py)),
                problem.map_or_else(|| py.None().into_bound(py), |p| p.into_bound(py).into_any()),
                error.map_or_else(|| py.None().into_bound(py), |e| e.into_bound(py).into_any()),
            ],
        )
    }

    /// The address that identifies the plan: two handles to one compiled type share it.
    #[getter]
    fn address(&self) -> usize {
        self.plan.address()
    }

    /// The type this plan was made for.
    #[getter]
    fn target<'py>(&self, py: Python<'py>) -> Bound<'py, PyAny> {
        // SAFETY: the target is borrowed from the plan, which this `Plan` keeps alive; the
        // `Bound` takes its own reference.
        unsafe { Bound::from_borrowed_ptr(py, self.plan.target().cast()) }
    }

    /// Another reference to the same plan.
    fn clone_handle(&self) -> Plan {
        Plan {
            plan: self.plan.clone(),
        }
    }
}

/// The plan of `target`, shared with `ox.json.decode`.
#[pyfunction]
fn plan_for(py: Python<'_>, target: &Bound<'_, PyAny>) -> PyResult<Plan> {
    // SAFETY: this thread is attached and `target` is a valid object, borrowed for the call.
    match unsafe { api().plan_for(target.as_ptr().cast()) } {
        Ok(plan) => Ok(Plan { plan }),
        Err(_) => Err(PyErr::fetch(py)),
    }
}

/// `plan_for` called with NULL, as a C host with a bug would: `(status, error)`.
#[pyfunction]
fn plan_for_null(py: Python<'_>) -> PyResult<(i32, Py<PyAny>)> {
    let mut out = core::ptr::null_mut();
    // SAFETY: the table refuses a NULL target without touching it.
    let status = unsafe { (api().table().plan_for)(core::ptr::null_mut(), &raw mut out) };
    let error = PyErr::take(py).map_or_else(|| py.None(), |error| error.into_value(py).into_any());
    Ok((status, error))
}

/// `decode` called with a NULL plan, as a C host with a bug would: `(status, error)`.
#[pyfunction]
fn decode_null_plan(py: Python<'_>, data: &Bound<'_, PyAny>) -> PyResult<(i32, Py<PyAny>)> {
    let data = bytes_of(data)?;
    let mut value = core::ptr::null_mut();
    let mut report = core::ptr::null_mut();
    // SAFETY: the table refuses a NULL plan without touching it; the other arguments are valid.
    let status = unsafe {
        (api().table().decode)(
            core::ptr::null(),
            1,
            data.as_ptr(),
            data.len(),
            core::ptr::null(),
            &raw mut value,
            &raw mut report,
        )
    };
    let error = PyErr::take(py).map_or_else(|| py.None(), |error| error.into_value(py).into_any());
    Ok((status, error))
}

/// Decodes the parts of one request, each with its own plan, mode and source, and builds one
/// problem when any of them is invalid (OX-4 through the ABI). `parts` is
/// `[(source, plan, data, mode)]` in order of priority.
#[pyfunction]
#[pyo3(signature = (parts, *, max_errors = 100, unknown = "model"))]
fn decode_request<'py>(
    py: Python<'py>,
    parts: &Bound<'py, PyList>,
    max_errors: u64,
    unknown: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let values = PyDict::new(py);
    let mut sources: Vec<String> = Vec::new();
    let mut reports: Vec<Report> = Vec::new();
    for part in parts.iter() {
        let (source, plan, data, mode): (String, Py<Plan>, Bound<'py, PyAny>, String) =
            part.extract()?;
        let options = decode_options(&mode, max_errors, None, unknown)?;
        match decode_through_table(py, &plan.get().plan, bytes_of(&data)?, &options)? {
            Ok(value) => values.set_item(source, value)?,
            Err(report) => {
                sources.push(source);
                reports.push(report);
            }
        }
    }
    if reports.is_empty() {
        return Ok(values);
    }
    let parts = sources.iter().map(String::as_str).zip(reports).collect();
    let combined = api()
        .combine_reports(parts, max_errors)
        .map_err(|status| PyValueError::new_err(api().status_message(status)))?;
    Err(HostProblem::new_err(problem_of(py, &combined)?))
}

/// Appends `obj` as JSON to a buffer of this module that already holds `prefix`, as the declared
/// type of `plan` when there is one.
#[pyfunction]
#[pyo3(signature = (obj, plan = None, *, sort_keys = false, indent = None, prefix = None))]
fn encode<'py>(
    py: Python<'py>,
    obj: &Bound<'py, PyAny>,
    plan: Option<&Plan>,
    sort_keys: bool,
    indent: Option<u32>,
    prefix: Option<&Bound<'py, PyBytes>>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut buffer =
        VecBuffer::from_vec(prefix.map_or_else(Vec::new, |bytes| bytes.as_bytes().to_vec()));
    let options = EncodeOptions { sort_keys, indent };
    // SAFETY: this thread is attached and `obj` is a valid object, borrowed for the call.
    unsafe {
        api().encode_json(
            obj.as_ptr().cast(),
            plan.map(|plan| &plan.plan),
            &options,
            &mut buffer,
        )
    }
    .map_err(|_| PyErr::fetch(py))?;
    Ok(PyBytes::new(py, buffer.as_slice()))
}

/// `encode` into a buffer with a chosen behaviour, as a C host calls it:
/// `(status, bytes, reserve_calls, error)`.
#[pyfunction]
#[pyo3(signature = (obj, plan = None, *, buffer = "grow", capacity = 0, prefix = None, flags = 0, indent = 0, struct_size = None, format = 1, buffer_struct_size = None))]
#[allow(clippy::too_many_arguments)]
fn encode_raw<'py>(
    py: Python<'py>,
    obj: &Bound<'py, PyAny>,
    plan: Option<&Plan>,
    buffer: &str,
    capacity: usize,
    prefix: Option<&Bound<'py, PyBytes>>,
    flags: u32,
    indent: u32,
    struct_size: Option<u32>,
    format: u32,
    buffer_struct_size: Option<u32>,
) -> PyResult<Bound<'py, PyTuple>> {
    let mode = match buffer {
        "grow" => BufferMode::Grow,
        "fixed" => BufferMode::Fixed,
        "refuse" => BufferMode::Refuse,
        "lie" => BufferMode::Lie,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown buffer mode {other:?}"
            )));
        }
    };
    let mut host = HostBuffer::new(
        mode,
        capacity,
        prefix.map_or(&[][..], |bytes| bytes.as_bytes()),
    );
    let mut options = OxEncodeOptions::new();
    options.flags = flags;
    options.indent = indent;
    if let Some(size) = struct_size {
        options.struct_size = size;
    }
    let raw: *mut OxBuffer = host.raw();
    if let Some(size) = buffer_struct_size {
        // SAFETY: `raw` points to the buffer struct of `host`, which lives through this function.
        unsafe { (&raw mut (*raw).struct_size).write(size) };
    }
    // SAFETY: this thread is attached; `obj` is a valid object; `plan` is alive; the options and
    // the buffer live through the call and the buffer keeps its contract (or breaks it on
    // purpose, which the table must survive).
    let status = unsafe {
        (api().table().encode)(
            format,
            obj.as_ptr().cast(),
            plan.map_or(core::ptr::null(), |plan| plan.plan.as_ptr()),
            &raw const options,
            raw,
        )
    };
    let error = PyErr::take(py).map(|error| error.into_value(py));
    PyTuple::new(
        py,
        [
            status.into_pyobject(py)?.into_any(),
            PyBytes::new(py, host.written()).into_any(),
            host.reserve_calls().into_pyobject(py)?.into_any(),
            error.map_or_else(|| py.None().into_bound(py), |e| e.into_bound(py).into_any()),
        ],
    )
}

/// `(major, minor, oxiform version)` of the table this module is using.
#[pyfunction]
fn abi_version() -> (u32, u32, &'static str) {
    let (major, minor) = api().version();
    (major, minor, api().oxiform_version())
}

/// Imports the capsule as a host built for `major.minor` would: raises `ImportError` when the
/// installed oxiform cannot serve it. Returns the `(major, minor)` it found.
#[pyfunction]
fn import_as(py: Python<'_>, major: u32, minor: u32) -> PyResult<(u32, u32)> {
    Ok(oxiform_abi::import_version(py, major, minor)?.version())
}

/// What a status code means, from the installed oxiform.
#[pyfunction]
fn status_message(status: i32) -> &'static str {
    api().status_message(Status(status))
}

/// The limits oxiform fills in: `"default"` or `"none"`.
#[pyfunction]
fn limits<'py>(py: Python<'py>, which: &str) -> PyResult<Bound<'py, PyDict>> {
    let raw = match which {
        "default" => api().limits_default(),
        "none" => api().limits_none(),
        other => {
            return Err(PyValueError::new_err(format!(
                "which must be \"default\" or \"none\", not {other:?}"
            )));
        }
    };
    let limits = PyDict::new(py);
    limits.set_item("max_bytes", raw.max_bytes)?;
    limits.set_item("max_depth", raw.max_depth)?;
    limits.set_item("max_items", raw.max_items)?;
    limits.set_item("max_str_len", raw.max_str_len)?;
    limits.set_item("max_int_digits", raw.max_int_digits)?;
    Ok(limits)
}

#[pymodule(gil_used = false)]
fn oxiform_abi_host(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    let api = oxiform_abi::import(py)?;
    API.get_or_init(|| api);
    module.add_class::<Plan>()?;
    module.add("HostProblem", py.get_type::<HostProblem>())?;
    module.add("OX_OK", OX_OK)?;
    module.add("OX_INVALID", OX_INVALID)?;
    module.add_function(wrap_pyfunction!(plan_for, module)?)?;
    module.add_function(wrap_pyfunction!(plan_for_null, module)?)?;
    module.add_function(wrap_pyfunction!(decode_null_plan, module)?)?;
    module.add_function(wrap_pyfunction!(decode_request, module)?)?;
    module.add_function(wrap_pyfunction!(encode, module)?)?;
    module.add_function(wrap_pyfunction!(encode_raw, module)?)?;
    module.add_function(wrap_pyfunction!(abi_version, module)?)?;
    module.add_function(wrap_pyfunction!(import_as, module)?)?;
    module.add_function(wrap_pyfunction!(status_message, module)?)?;
    module.add_function(wrap_pyfunction!(limits, module)?)?;
    Ok(())
}
