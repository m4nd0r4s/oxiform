//! Plan handles: the compiled types of the process, reference counted for a host.

use std::sync::{Arc, OnceLock};

use oxiform_abi::{OX_INVALID_ARGUMENT, OX_OK, OX_PYTHON_ERROR, OxPlan, OxPyObject};
use pyo3::prelude::*;

use super::calls::{attached, invalid_argument, raise};
use crate::compiled::{CompiledType, compiled_for};
use crate::encode::declared::{DeclaredPlan, declared_for};

/// What an `OxPlan` points to. The compiled type is the one in the cache of `ox.json.decode`
/// (an `Arc` clone), so a plan of a host is never a second copy.
pub(super) struct PlanHandle {
    target: Py<PyAny>,
    pub(super) compiled: Arc<CompiledType>,
    declared: OnceLock<Arc<DeclaredPlan>>,
}

impl PlanHandle {
    /// The plan of encoding as the declared type, read on first use and shared with the cache of
    /// `ox.json.encode(obj, type=...)`.
    pub(super) fn declared(&self, py: Python<'_>) -> PyResult<Arc<DeclaredPlan>> {
        if let Some(declared) = self.declared.get() {
            return Ok(declared.clone());
        }
        let declared = declared_for(self.target.bind(py))?;
        Ok(self.declared.get_or_init(|| declared).clone())
    }
}

/// # Safety
///
/// See [`oxiform_abi::OxiformApi::plan_for`].
pub(super) unsafe extern "C" fn plan_for(target: *mut OxPyObject, out: *mut *mut OxPlan) -> i32 {
    attached(|py| {
        if out.is_null() {
            return invalid_argument(
                py,
                "plan_for was given a NULL out pointer; pass the address of a *mut OxPlan",
            );
        }
        // SAFETY: `out` is non-null and the caller guarantees it is writable.
        unsafe { out.write(core::ptr::null_mut()) };
        if target.is_null() {
            return invalid_argument(
                py,
                "plan_for was given a NULL type; pass a borrowed PyObject* of a class or type expression",
            );
        }
        // SAFETY: the caller guarantees `target` is a valid `PyObject*` that stays alive during
        // the call, and the thread is attached.
        let target = unsafe { Bound::from_borrowed_ptr(py, target.cast()) };
        match compiled_for(&target) {
            Ok(compiled) => {
                let handle = Arc::new(PlanHandle {
                    target: target.unbind(),
                    compiled,
                    declared: OnceLock::new(),
                });
                // SAFETY: `out` is writable; the pointer is the one reference `Arc::into_raw` gives.
                unsafe { out.write(Arc::into_raw(handle).cast_mut().cast::<OxPlan>()) };
                OX_OK
            }
            Err(error) => raise(py, OX_PYTHON_ERROR, error),
        }
    })
}

/// # Safety
///
/// `plan` must be null or a live plan of this oxiform.
pub(super) unsafe extern "C" fn plan_retain(plan: *const OxPlan) {
    if !plan.is_null() {
        // SAFETY: the caller guarantees `plan` is a live `Arc<PlanHandle>` made by `plan_for`.
        unsafe { Arc::increment_strong_count(plan.cast::<PlanHandle>()) };
    }
}

/// # Safety
///
/// `plan` must be null or a live plan of this oxiform, of which the caller owns the reference it
/// gives up.
pub(super) unsafe extern "C" fn plan_release(plan: *const OxPlan) {
    if plan.is_null() {
        return;
    }
    // SAFETY: the caller owns one reference of a live `Arc<PlanHandle>` and gives it up. Dropping
    // the last one frees Python objects, so the thread attaches itself first.
    Python::attach(|_| drop(unsafe { Arc::from_raw(plan.cast::<PlanHandle>()) }));
}

/// # Safety
///
/// `plan` must be null or a live plan of this oxiform.
pub(super) unsafe extern "C" fn plan_target(plan: *const OxPlan) -> *mut OxPyObject {
    if plan.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the caller guarantees `plan` is live; the target is owned by it.
    unsafe {
        (*plan.cast::<PlanHandle>())
            .target
            .as_ptr()
            .cast::<OxPyObject>()
    }
}

/// The handle behind a plan pointer.
///
/// # Safety
///
/// `plan` must be a live plan of this oxiform that outlives `'a`.
pub(super) unsafe fn handle<'a>(plan: *const OxPlan) -> Result<&'a PlanHandle, i32> {
    if plan.is_null() {
        return Err(OX_INVALID_ARGUMENT);
    }
    // SAFETY: forwarded from the caller.
    Ok(unsafe { &*plan.cast::<PlanHandle>() })
}
