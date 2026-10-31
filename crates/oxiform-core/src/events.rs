//! The events every format reader emits. The executor consumes them without knowing which
//! format produced them, so one executor serves JSON, YAML, TOML and MessagePack.

use std::borrow::Cow;
use std::fmt;

/// One step of a document, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub enum Event<'input> {
    MapStart,
    MapEnd,
    SequenceStart,
    SequenceEnd,
    /// A key inside a map.
    Key(Cow<'input, str>),
    String(Cow<'input, str>),
    /// A number exactly as written in the input, so integers of any size and decimals keep
    /// every digit until the target type decides how to read them.
    Number(&'input str),
    Bool(bool),
    Null,
    /// The document is over. Returned after the top-level value, and again on every later call.
    End,
}

/// Why a reader stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaderErrorKind {
    /// The input is not well formed in its format.
    Syntax,
    /// Containers are nested deeper than the configured limit.
    DepthLimit { limit: usize },
}

/// A reader failure with its place in the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderError {
    pub kind: ReaderErrorKind,
    pub message: String,
    /// Byte offset in the input.
    pub offset: usize,
    /// 1-based line, for text formats.
    pub line: usize,
    /// 1-based column in characters, for text formats.
    pub column: usize,
}

impl fmt::Display for ReaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at line {}, column {}",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for ReaderError {}

/// A pull reader over one document.
pub trait EventReader<'input> {
    /// The next event, or the reason the input cannot be read further.
    fn next_event(&mut self) -> Result<Event<'input>, ReaderError>;

    /// The byte offset where the last returned event starts.
    fn offset(&self) -> usize;
}
