//! Compiled types, cached per target type for the life of the process.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use oxiform_core::compiler::compile;
use oxiform_core::plan::Plan;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyTuple};

use crate::description::{Runtime, read_description};
use crate::python_objects::{describe, schema_error};

/// A plan and what the sink needs for its structures.
pub(crate) struct CompiledType {
    pub(crate) plan: Plan,
    pub(crate) runtime: Runtime,
}

/// Holds a compiled type in the cache dict.
#[pyclass(frozen, module = "oxiform._oxiform")]
pub(crate) struct PlanHandle {
    compiled: Arc<CompiledType>,
}

static PLANS: PyOnceLock<Py<PyDict>> = PyOnceLock::new();

/// How many types were compiled in this process: the embedding ABI shares the cache of
/// `ox.json.decode`, so asking for a plan of a known type compiles nothing.
static COMPILATIONS: AtomicU64 = AtomicU64::new(0);

/// The compiled form of `target`, compiling it on first use.
///
/// One plan per type for the life of the process: the cache never evicts, and when threads race to
/// compile the same type (free-threaded builds) the first to insert wins and every caller gets its
/// plan (the others' work is dropped). A target that is not hashable (an `Annotated` with
/// unhashable metadata) is not cached and compiles at every call: a host keeps the handle.
pub(crate) fn compiled_for(target: &Bound<'_, PyAny>) -> PyResult<Arc<CompiledType>> {
    let py = target.py();
    let cache = PLANS.get_or_init(py, || PyDict::new(py).unbind()).bind(py);
    let cached = match cache.get_item(target) {
        Ok(cached) => cached,
        // Unhashable targets (Annotated with unhashable metadata) are compiled every time.
        Err(_) => return compile_target(target).map(Arc::new),
    };
    if let Some(handle) = cached {
        return Ok(handle.cast::<PlanHandle>()?.get().compiled.clone());
    }
    let compiled = Arc::new(compile_target(target)?);
    let handle = Bound::new(
        py,
        PlanHandle {
            compiled: compiled.clone(),
        },
    )?;
    // `setdefault` is atomic: of two threads that compiled the same type, both get the plan of
    // the first to insert.
    let winner = cache.call_method1("setdefault", (target, handle))?;
    Ok(winner.cast::<PlanHandle>()?.get().compiled.clone())
}

/// Compiles the plans of `types` now instead of on first use.
#[pyfunction]
#[pyo3(signature = (*types))]
pub(crate) fn warmup(types: &Bound<'_, PyTuple>) -> PyResult<()> {
    for target in types.iter() {
        compiled_for(&target)?;
    }
    Ok(())
}

fn compile_target(target: &Bound<'_, PyAny>) -> PyResult<CompiledType> {
    let py = target.py();
    COMPILATIONS.fetch_add(1, Ordering::Relaxed);
    let description = describe(py)?.call1((target,))?;
    let (description, runtime) = read_description(&description)?;
    match compile(&description) {
        Ok(plan) => Ok(CompiledType { plan, runtime }),
        Err(error) => Err(PyErr::from_type(
            schema_error(py)?.clone(),
            error.to_string(),
        )),
    }
}

/// `(compilations, cached types)` of the plan cache, to tell that a host shares it.
#[pyfunction]
pub(crate) fn _plan_stats(py: Python<'_>) -> (u64, usize) {
    let cached = PLANS.get(py).map_or(0, |cache| cache.bind(py).len());
    (COMPILATIONS.load(Ordering::Relaxed), cached)
}

/// The address of the cached compiled type of `target`: two calls that return the same number
/// share one plan, which a test of racing threads checks.
#[pyfunction]
pub(crate) fn _plan_id(target: &Bound<'_, PyAny>) -> PyResult<usize> {
    Ok(Arc::as_ptr(&compiled_for(target)?) as usize)
}
