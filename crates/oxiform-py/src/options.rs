//! The options of a decode call: `ox.Limits`, `mode` and `max_errors`.

use oxiform_core::limits::Limits;
use oxiform_core::options::{Mode, Options};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// `ox.Limits`: what protects a decode against hostile input (section 7 of the spec).
#[pyclass(frozen, eq, hash, name = "Limits", module = "oxiform")]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PyLimits {
    #[pyo3(get)]
    max_bytes: usize,
    #[pyo3(get)]
    max_depth: usize,
    #[pyo3(get)]
    max_items: usize,
    #[pyo3(get)]
    max_str_len: usize,
    #[pyo3(get)]
    max_int_digits: usize,
}

#[pymethods]
impl PyLimits {
    #[new]
    #[pyo3(signature = (
        *,
        max_bytes = Limits::DEFAULT.max_bytes,
        max_depth = Limits::DEFAULT.max_depth,
        max_items = Limits::DEFAULT.max_items,
        max_str_len = Limits::DEFAULT.max_str_len,
        max_int_digits = Limits::DEFAULT.max_int_digits,
    ))]
    fn new(
        max_bytes: usize,
        max_depth: usize,
        max_items: usize,
        max_str_len: usize,
        max_int_digits: usize,
    ) -> Self {
        Self {
            max_bytes,
            max_depth,
            max_items,
            max_str_len,
            max_int_digits,
        }
    }

    /// No limits, for trusted input.
    #[staticmethod]
    fn none() -> Self {
        Self::from(Limits::none())
    }

    fn __repr__(&self) -> String {
        format!(
            "Limits(max_bytes={}, max_depth={}, max_items={}, max_str_len={}, max_int_digits={})",
            self.max_bytes, self.max_depth, self.max_items, self.max_str_len, self.max_int_digits
        )
    }
}

impl From<Limits> for PyLimits {
    fn from(limits: Limits) -> Self {
        Self {
            max_bytes: limits.max_bytes,
            max_depth: limits.max_depth,
            max_items: limits.max_items,
            max_str_len: limits.max_str_len,
            max_int_digits: limits.max_int_digits,
        }
    }
}

/// The options of one decode call, from the arguments Python passed.
pub(crate) fn decode_options(
    mode: &str,
    limits: Option<&PyLimits>,
    max_errors: usize,
) -> PyResult<Options> {
    let mode = match mode {
        "strict" => Mode::Strict,
        "lax" => Mode::Lax,
        other => {
            return Err(PyValueError::new_err(format!(
                "mode must be \"strict\" or \"lax\", not {other:?}"
            )));
        }
    };
    let limits = match limits {
        Some(limits) => Limits {
            max_bytes: limits.max_bytes,
            max_depth: limits.max_depth,
            max_items: limits.max_items,
            max_str_len: limits.max_str_len,
            max_int_digits: limits.max_int_digits,
            max_errors,
        },
        None => Limits {
            max_errors,
            ..Limits::DEFAULT
        },
    };
    Ok(Options { mode, limits })
}
