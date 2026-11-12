//! How one run reads its input: how strictly it coerces, and which limits apply.

use crate::limits::Limits;

/// How strictly values are coerced to the target type (D10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The input must already have the type: `"42"` is not an `int`. For typed formats.
    #[default]
    Strict,
    /// Text is read as the type the field asks for: `"42"` is an `int`, `"yes"` is a `bool`.
    /// For text that comes from environment variables, forms and query strings.
    Lax,
}

/// Everything a run needs besides the plan, the reader and the sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub mode: Mode,
    /// What protects the run against hostile input.
    pub limits: Limits,
    /// Issues collected before the run stops (at least 1; 1 is "fail fast"). It is here and not
    /// in `limits` because it shapes the report and protects nothing: `Limits` holds sizes
    /// of the input, and a caller that turns the limits off still wants a bounded report.
    pub max_errors: usize,
}

impl Options {
    /// Issues collected when the caller does not say.
    pub const DEFAULT_MAX_ERRORS: usize = 100;
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            limits: Limits::default(),
            max_errors: Self::DEFAULT_MAX_ERRORS,
        }
    }
}
