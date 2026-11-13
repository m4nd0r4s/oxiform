//! The exporting half of the embedding ABI: everything oxiform does for a host that needs no
//! Python. `oxiform-py` adds the functions that run with the interpreter (plans, decode, encode)
//! and the capsule; this crate holds the reports as C views, the options and the output buffer,
//! so the `unsafe` code that walks raw pointers can be tested (and run under Miri) without an
//! interpreter.

mod buffer;
mod options;
mod report;

pub use buffer::{append, check_buffer};
pub use options::{EncodeParams, PairsParams, decode_options, encode_options, pairs_options};
pub use report::{
    into_report_pointer, limits_default, limits_none, report_combine, report_free,
    report_in_source, report_view, status_message,
};

use oxiform_abi::OX_INTERNAL;

/// A failed call: the status to return and a sentence that says what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub status: i32,
    pub message: String,
}

impl Error {
    pub fn new(status: i32, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

/// Runs `call`, turning a panic into [`OX_INTERNAL`]: nothing unwinds across the boundary.
pub fn guard(call: impl FnOnce() -> i32) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)).unwrap_or(OX_INTERNAL)
}
