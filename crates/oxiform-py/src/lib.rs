//! The `oxiform._oxiform` extension module.

mod abi;
mod compiled;
mod decode;
mod description;
mod encode;
mod errors;
mod options;
mod pairs;
mod python_objects;
mod sink;
mod slots;
mod validate;

use pyo3::prelude::*;

#[pymodule(gil_used = false)]
fn _oxiform(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add_function(wrap_pyfunction!(compiled::warmup, module)?)?;
    module.add_function(wrap_pyfunction!(compiled::_plan_stats, module)?)?;
    module.add_function(wrap_pyfunction!(compiled::_plan_id, module)?)?;
    abi::export(module)?;
    module.add_function(wrap_pyfunction!(decode::json_decode, module)?)?;
    module.add_function(wrap_pyfunction!(pairs::urlencoded_decode, module)?)?;
    module.add_function(wrap_pyfunction!(pairs::pairs_decode, module)?)?;
    module.add_function(wrap_pyfunction!(encode::json::json_encode, module)?)?;
    module.add_class::<encode::json::JsonEncoder>()?;
    module.add_class::<decode::JsonDecoder>()?;
    module.add_class::<compiled::PlanHandle>()?;
    module.add_class::<options::PyLimits>()?;
    Ok(())
}
