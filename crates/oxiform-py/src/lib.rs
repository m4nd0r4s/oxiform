//! The `oxiform._oxiform` extension module.

mod compiled;
mod decode;
mod description;
mod encode;
mod errors;
mod python_objects;
mod sink;
mod slots;

use pyo3::prelude::*;

#[pymodule(gil_used = false)]
fn _oxiform(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add_function(wrap_pyfunction!(decode::json_decode, module)?)?;
    module.add_function(wrap_pyfunction!(encode::json::json_encode, module)?)?;
    module.add_class::<encode::json::JsonEncoder>()?;
    module.add_class::<decode::JsonDecoder>()?;
    module.add_class::<compiled::PlanHandle>()?;
    Ok(())
}
