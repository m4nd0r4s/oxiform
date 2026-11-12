//! Limits that protect against hostile input. All are on by default. The number of issues
//! collected is not one of them: it is `Options::max_errors`, a choice about reporting.

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
    };

    /// The most nesting levels any run reads, whatever `max_depth` says. The executor reads one
    /// level per call and its stack use depends on the type: about 550 bytes per level for
    /// untyped data, 1.3 to 1.4 KB for a recursive model (through `Optional`, a union, a `list`
    /// or a `dict`) and 2 KB through an `ox.Untagged` union. A thread has as little as 512 KiB
    /// of stack on macOS, where the worst of these crashes at 256 levels, so the ceiling is half
    /// of that: every shape measured uses at most half the stack at 128 levels. Deeper input is
    /// a `limit_exceeded` error, never a crash.
    pub const DEPTH_CEILING: usize = 128;

    /// No size limits, for trusted input, except that nesting stops at [`Limits::DEPTH_CEILING`]
    /// and integers keep the digit limit of the sink (CPython has its own).
    pub fn none() -> Self {
        Self {
            max_bytes: usize::MAX,
            max_depth: Self::DEPTH_CEILING,
            max_items: usize::MAX,
            max_str_len: usize::MAX,
            max_int_digits: usize::MAX,
        }
    }
}
