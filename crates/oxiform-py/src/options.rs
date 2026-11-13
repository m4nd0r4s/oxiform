//! The options of a decode call: `ox.Limits`, `mode`, `max_errors` and `unknown`.

use oxiform_core::description::UnknownFields;
use oxiform_core::limits::Limits;
use oxiform_core::options::{Mode, Options};
use pyo3::exceptions::{PyOverflowError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBool;

/// `ox.Limits`: what protects a decode against hostile input (section 7 of the spec).
///
/// Every size is a whole number of at least 0. `max_depth` is at most `Limits.MAX_DEPTH` (128, the default):
/// the reader keeps that ceiling whatever it is given, so that deeply nested input is an error
/// and never a crash, and `Limits.none()` has that depth too.
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
        #[pyo3(from_py_with = max_bytes_argument)] max_bytes: usize,
        #[pyo3(from_py_with = max_depth_argument)] max_depth: usize,
        #[pyo3(from_py_with = max_items_argument)] max_items: usize,
        #[pyo3(from_py_with = max_str_len_argument)] max_str_len: usize,
        #[pyo3(from_py_with = max_int_digits_argument)] max_int_digits: usize,
    ) -> Self {
        Self {
            max_bytes,
            max_depth: max_depth.min(Limits::DEPTH_CEILING),
            max_items,
            max_str_len,
            max_int_digits,
        }
    }

    /// The most nesting levels any decode reads.
    #[classattr]
    #[pyo3(name = "MAX_DEPTH")]
    fn max_depth_ceiling() -> usize {
        Limits::DEPTH_CEILING
    }

    /// No limits, for trusted input. Nesting still stops at `Limits.MAX_DEPTH` levels and
    /// integers at the digit limit of Python (`sys.get_int_max_str_digits()`).
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

/// A size argument of `Limits`: a whole number from 0 to the largest `usize`. Anything else
/// that is a number says so and points at `Limits.none()`, instead of a bare `OverflowError`.
fn size_argument(name: &str, value: &Bound<'_, PyAny>) -> PyResult<usize> {
    value.extract::<usize>().map_err(|error| {
        if error.is_instance_of::<PyOverflowError>(value.py()) {
            PyValueError::new_err(format!(
                "{name} must be a whole number from 0 to {}, not {value}; \
                 use ox.Limits.none() to turn the limits off",
                usize::MAX
            ))
        } else {
            error
        }
    })
}

/// The `max_errors` argument of a decode call: a whole number from 1 to the largest `usize`.
pub(crate) fn max_errors_argument(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    if value.is_instance_of::<PyBool>() {
        return Err(PyTypeError::new_err(
            "max_errors must be a whole number, not bool; use max_errors=1 to stop at the first error",
        ));
    }
    let count = value.extract::<usize>().map_err(|error| {
        if error.is_instance_of::<PyOverflowError>(value.py()) {
            PyValueError::new_err(format!(
                "max_errors must be a whole number from 1 to {}, not {value}",
                usize::MAX
            ))
        } else {
            error
        }
    })?;
    if count == 0 {
        return Err(PyValueError::new_err(
            "max_errors must be at least 1, not 0; use max_errors=1 to stop at the first error",
        ));
    }
    Ok(count)
}

macro_rules! size_argument {
    ($function:ident, $name:literal) => {
        fn $function(value: &Bound<'_, PyAny>) -> PyResult<usize> {
            size_argument($name, value)
        }
    };
}

size_argument!(max_bytes_argument, "max_bytes");
size_argument!(max_depth_argument, "max_depth");
size_argument!(max_items_argument, "max_items");
size_argument!(max_str_len_argument, "max_str_len");
size_argument!(max_int_digits_argument, "max_int_digits");

/// The options of one decode call, from the arguments Python passed.
pub(crate) fn decode_options(
    mode: &str,
    limits: Option<&PyLimits>,
    max_errors: usize,
    unknown: Option<&str>,
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
        },
        None => Limits::DEFAULT,
    };
    let unknown = match unknown {
        None => None,
        Some("ignore") => Some(UnknownFields::Ignore),
        Some("forbid") => Some(UnknownFields::Forbid),
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "unknown must be \"ignore\", \"forbid\" or None, not {other:?}; \
                 None leaves it to the unknown= option of each model"
            )));
        }
    };
    Ok(Options {
        mode,
        limits,
        max_errors,
        unknown,
    })
}
