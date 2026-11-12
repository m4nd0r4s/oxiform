//! Limits that protect against hostile input. All are on by default.

/// Limits of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes per document.
    pub max_bytes: usize,
    /// Nesting levels, at most [`Limits::DEPTH_CEILING`].
    pub max_depth: usize,
    /// Items per collection, or keys per map.
    pub max_items: usize,
    /// Bytes per string.
    pub max_str_len: usize,
    /// Digits per integer, as in CPython.
    pub max_int_digits: usize,
    /// Issues collected before stopping.
    pub max_errors: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Limits {
    /// The limits that apply when none are given.
    pub const DEFAULT: Self = Self {
        max_bytes: 64 * 1024 * 1024,
        max_depth: 128,
        max_items: 1_000_000,
        max_str_len: 16 * 1024 * 1024,
        max_int_digits: 4300,
        max_errors: 100,
    };

    /// The most nesting levels any run reads, whatever `max_depth` says. The executor reads one
    /// level per call, about 600 bytes of stack each, so this keeps a run inside the 512 KiB
    /// stack that threads have on macOS, with room to spare. Deeper input is a
    /// `limit_exceeded` error, never a crash.
    pub const DEPTH_CEILING: usize = 512;

    /// No size limits, for trusted input, except that nesting stops at [`Limits::DEPTH_CEILING`]
    /// and integers keep the digit limit of the sink (CPython has its own). The number of
    /// errors collected stays bounded.
    pub fn none() -> Self {
        Self {
            max_bytes: usize::MAX,
            max_depth: Self::DEPTH_CEILING,
            max_items: usize::MAX,
            max_str_len: usize::MAX,
            max_int_digits: usize::MAX,
            max_errors: Self::DEFAULT.max_errors,
        }
    }
}
