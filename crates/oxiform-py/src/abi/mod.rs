//! The embedding ABI: the table of functions a native extension of another cdylib calls to use
//! oxiform without linking it (`oxiform-abi` has the types; section 9.5 of the specification
//! has the rules).
//!
//! `oxiform-abi-export` has the parts that need no interpreter (reports, options, the output
//! buffer); this module has the ones that do (plans, decode, encode) and the capsule.

mod calls;
mod plan;

use std::ptr::NonNull;

use oxiform_abi::{
    ABI_MAJOR, ABI_MINOR, CAPSULE_ATTRIBUTE, CAPSULE_NAME, MAJORS_ATTRIBUTE, OxStr, OxiformApi,
};
use pyo3::prelude::*;
use pyo3::types::PyCapsule;

/// The table, which lives for the whole process.
struct Table(OxiformApi);

// SAFETY: the table is immutable, and holds function pointers and a pointer to a static string.
unsafe impl Sync for Table {}

static TABLE: Table = Table(OxiformApi {
    abi_major: ABI_MAJOR,
    abi_minor: ABI_MINOR,
    struct_size: size_of::<OxiformApi>() as u32,
    reserved: 0,
    oxiform_version: OxStr::from_str(env!("CARGO_PKG_VERSION")),
    status_message: oxiform_abi_export::status_message,
    limits_default: oxiform_abi_export::limits_default,
    limits_none: oxiform_abi_export::limits_none,
    plan_for: plan::plan_for,
    plan_retain: plan::plan_retain,
    plan_release: plan::plan_release,
    plan_target: plan::plan_target,
    decode: calls::decode,
    encode: calls::encode,
    report_view: oxiform_abi_export::report_view,
    report_in_source: oxiform_abi_export::report_in_source,
    report_combine: oxiform_abi_export::report_combine,
    report_free: oxiform_abi_export::report_free,
    decode_pairs: calls::decode_pairs,
});

/// Adds the capsule, and the versions it can be asked for, to the module.
pub(crate) fn export(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    // SAFETY: the pointer is to a static table, valid for the life of the process, with no
    // destructor to run; `Table` is `#[repr(transparent)]`-like: its only field is the table, so
    // the pointer is the address of an `OxiformApi`.
    let capsule =
        unsafe { PyCapsule::new_with_pointer(py, NonNull::from(&TABLE.0).cast(), CAPSULE_NAME) }?;
    module.add(CAPSULE_ATTRIBUTE, capsule)?;
    module.add(MAJORS_ATTRIBUTE, (ABI_MAJOR,))?;
    module.add("ABI_VERSION", (ABI_MAJOR, ABI_MINOR))?;
    Ok(())
}
