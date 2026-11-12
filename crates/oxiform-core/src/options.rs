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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub mode: Mode,
    pub limits: Limits,
}
