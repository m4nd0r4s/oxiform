//! Compiled types, cached per target type for the life of the process.

use std::sync::Arc;

use oxiform_core::compiler::compile;
use oxiform_core::plan::Plan;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyTuple};

use crate::description::{StructRuntime, read_description};
use crate::python_objects::{describe, schema_error};

/// A plan and what the sink needs for its structures.
pub(crate) struct CompiledType {
    pub(crate) plan: Plan,
    pub(crate) structs: Vec<StructRuntime>,
}

/// Holds a compiled type in the cache dict.
#[pyclass(frozen, module = "oxiform._oxiform")]
pub(crate) struct PlanHandle {
    compiled: Arc<CompiledType>,
}

static PLANS: PyOnceLock<Py<PyDict>> = PyOnceLock::new();

/// The compiled form of `target`, compiling it on first use.
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
    cache.set_item(target, handle)?;
    Ok(compiled)
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
    let description = describe(py)?.call1((target,))?;
    let (description, structs) = read_description(&description)?;
    match compile(&description) {
        Ok(plan) => Ok(CompiledType { plan, structs }),
        Err(error) => Err(PyErr::from_type(
            schema_error(py)?.clone(),
            error.to_string(),
        )),
    }
}
