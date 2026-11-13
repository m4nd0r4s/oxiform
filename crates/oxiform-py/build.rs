//! Emits the `Py_3_xx` cfgs of the Python being built for, as PyO3 does for its own code.

fn main() {
    pyo3_build_config::use_pyo3_cfgs();
}
