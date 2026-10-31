//! Limits that protect against hostile input. All are on by default.

/// Limits of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes per document.
    pub max_bytes: usize,
    /// Nesting levels.
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
        Self {
            max_bytes: 64 * 1024 * 1024,
            max_depth: 128,
            max_items: 1_000_000,
            max_str_len: 16 * 1024 * 1024,
            max_int_digits: 4300,
            max_errors: 100,
        }
    }
}

impl Limits {
    /// No size limits, for trusted input. The number of errors collected stays bounded.
    pub fn none() -> Self {
        Self {
            max_bytes: usize::MAX,
            max_depth: usize::MAX,
            max_items: usize::MAX,
            max_str_len: usize::MAX,
            max_int_digits: usize::MAX,
            max_errors: Self::default().max_errors,
        }
    }
}
