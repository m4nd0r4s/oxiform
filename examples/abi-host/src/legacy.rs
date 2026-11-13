//! A host built for ABI 1.0, as a host that was compiled before 1.1 existed still sees the table.
//!
//! `LegacyTable` is the table of 1.0 as it was released, copied here and never edited: it has no
//! `decode_pairs`. The functions below find the capsule of the installed oxiform and use it only
//! through this struct, as that host does: they prove that the 1.1 table starts with the 1.0 one,
//! that the version check of 1.0 accepts it, and that the 1.0 calls behave as they did.

use oxiform_abi::{
    OX_FORMAT_JSON, OX_INVALID, OX_OK, OxDecodeOptions, OxLimits, OxPlan, OxPyObject, OxReport,
    OxReportPart, OxReportView, OxStr, OxiformApiHeader, check_header,
};
use pyo3::exceptions::{PyImportError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

/// The function table of ABI 1.0.
#[repr(C)]
struct LegacyTable {
    abi_major: u32,
    abi_minor: u32,
    struct_size: u32,
    reserved: u32,
    oxiform_version: OxStr,
    status_message: unsafe extern "C" fn(i32) -> OxStr,
    limits_default: unsafe extern "C" fn(*mut OxLimits) -> i32,
    limits_none: unsafe extern "C" fn(*mut OxLimits) -> i32,
    plan_for: unsafe extern "C" fn(*mut OxPyObject, *mut *mut OxPlan) -> i32,
    plan_retain: unsafe extern "C" fn(*const OxPlan),
    plan_release: unsafe extern "C" fn(*const OxPlan),
    plan_target: unsafe extern "C" fn(*const OxPlan) -> *mut OxPyObject,
    decode: unsafe extern "C" fn(
        *const OxPlan,
        u32,
        *const u8,
        usize,
        *const OxDecodeOptions,
        *mut *mut OxPyObject,
        *mut *mut OxReport,
    ) -> i32,
    encode: unsafe extern "C" fn() -> i32,
    report_view: unsafe extern "C" fn(*const OxReport) -> *const OxReportView,
    report_in_source: unsafe extern "C" fn(*mut OxReport, OxStr) -> i32,
    report_combine:
        unsafe extern "C" fn(*const OxReportPart, usize, u64, *mut *mut OxReport) -> i32,
    report_free: unsafe extern "C" fn(*mut OxReport),
}

/// The table of the installed oxiform, read as a host built for 1.0 reads it: the version check
/// of 1.0 (`1.0` or newer, a table at least as long as `LegacyTable`), then the struct.
fn legacy_table(py: Python<'_>) -> PyResult<&'static LegacyTable> {
    let api = crate::api();
    let header = (api.table() as *const oxiform_abi::OxiformApi).cast::<OxiformApiHeader>();
    // SAFETY: the table starts with its header and lives for the process.
    let found = unsafe { header.read() };
    check_header(found, 1, 0, size_of::<LegacyTable>())
        .map_err(|error| PyImportError::new_err(error.to_string()))?;
    let _ = py;
    // SAFETY: the check above says the table has at least `size_of::<LegacyTable>()` bytes, and the
    // layout of 1.1 starts with the layout of 1.0 (a test compares every offset).
    Ok(unsafe { &*header.cast::<LegacyTable>() })
}

/// Decodes `data` as `target` the way a host of 1.0 does, with options sized for 1.0, and returns
/// `(status, value)`; an invalid document is `(OX_INVALID, None)` after freeing the report.
#[pyfunction]
#[pyo3(signature = (target, data, *, lax = false))]
pub fn legacy_decode<'py>(
    py: Python<'py>,
    target: &Bound<'py, PyAny>,
    data: &Bound<'py, PyBytes>,
    lax: bool,
) -> PyResult<(i32, Option<Bound<'py, PyAny>>)> {
    let table = legacy_table(py)?;
    let mut plan = core::ptr::null_mut();
    // SAFETY: this thread is attached and `target` is a valid object, borrowed for the call.
    let status = unsafe { (table.plan_for)(target.as_ptr().cast(), &raw mut plan) };
    if status != OX_OK {
        return Err(PyErr::fetch(py));
    }
    let mut options = OxDecodeOptions::new();
    options.struct_size = OxDecodeOptions::SIZE_V1_0 as u32;
    options.mode = u32::from(lax);
    let bytes = data.as_bytes();
    let mut value = core::ptr::null_mut();
    let mut report = core::ptr::null_mut();
    // SAFETY: the plan is live; the data and options live through the call; the outs are valid.
    let status = unsafe {
        (table.decode)(
            plan,
            OX_FORMAT_JSON,
            bytes.as_ptr(),
            bytes.len(),
            &raw const options,
            &raw mut value,
            &raw mut report,
        )
    };
    let outcome = match status {
        OX_OK => {
            // SAFETY: a new reference the table gave us.
            Ok((
                status,
                Some(unsafe { Bound::from_owned_ptr(py, value.cast()) }),
            ))
        }
        OX_INVALID => {
            // SAFETY: a report the table gave us, freed once.
            unsafe { (table.report_free)(report) };
            Ok((status, None))
        }
        _ => Err(PyErr::fetch(py)),
    };
    // SAFETY: the one reference `plan_for` gave us.
    unsafe { (table.plan_release)(plan) };
    outcome
}

/// A 1.0 host's version check against a table that exports `(major, minor)` with `size` bytes.
#[pyfunction]
pub fn legacy_check(major: u32, minor: u32, size: usize) -> PyResult<()> {
    let header = OxiformApiHeader {
        abi_major: major,
        abi_minor: minor,
        struct_size: u32::try_from(size).map_err(|_| PyValueError::new_err("size is too large"))?,
        reserved: 0,
    };
    check_header(header, 1, 0, size_of::<LegacyTable>())
        .map_err(|error| PyImportError::new_err(error.to_string()))
}

/// The size of the table of 1.0 as this host knows it.
#[pyfunction]
pub fn legacy_table_size() -> usize {
    size_of::<LegacyTable>()
}
