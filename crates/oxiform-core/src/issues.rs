//! Validation issues: what went wrong, where, and with which input.

use std::fmt::Write;

use crate::description::ChoiceValue;

/// The stable code of an issue. Codes are public API; messages are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    Syntax,
    Type,
    Missing,
    UnknownField,
    DuplicateKey,
    TooSmall,
    TooLarge,
    TooShort,
    TooLong,
    Pattern,
    Format,
    Literal,
    Enum,
    Union,
    Tag,
    Timezone,
    LimitExceeded,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Type => "type",
            Self::Missing => "missing",
            Self::UnknownField => "unknown_field",
            Self::DuplicateKey => "duplicate_key",
            Self::TooSmall => "too_small",
            Self::TooLarge => "too_large",
            Self::TooShort => "too_short",
            Self::TooLong => "too_long",
            Self::Pattern => "pattern",
            Self::Format => "format",
            Self::Literal => "literal",
            Self::Enum => "enum",
            Self::Union => "union",
            Self::Tag => "tag",
            Self::Timezone => "timezone",
            Self::LimitExceeded => "limit_exceeded",
        }
    }
}

/// One step of a path into the input: a key (as written in the input) or an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

/// A value in the context of an issue.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
    Integers(Vec<i64>),
    Texts(Vec<String>),
    /// Values of mixed types, for the values a `Literal` allows.
    Values(Vec<ChoiceValue>),
    /// The issues of each variant of a union that was tried, by variant name.
    Variants(Vec<(String, Vec<Issue>)>),
}

/// One problem found in the input.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub code: ErrorCode,
    pub path: Vec<PathSegment>,
    pub message: String,
    /// The received value as text, truncated; `None` for containers.
    pub input: Option<String>,
    pub context: Vec<(&'static str, ContextValue)>,
}

impl Issue {
    /// The path as a JSON Pointer (RFC 6901): `/addresses/0/street`.
    pub fn pointer(&self) -> String {
        let mut pointer = String::new();
        for segment in &self.path {
            pointer.push('/');
            match segment {
                PathSegment::Key(key) => {
                    pointer.push_str(&key.replace('~', "~0").replace('/', "~1"))
                }
                PathSegment::Index(index) => {
                    let _ = write!(pointer, "{index}");
                }
            }
        }
        pointer
    }
}

/// Whether the input was read to the end, or reading stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    /// The input was well formed but did not match the type.
    Validation,
    /// Reading stopped: malformed input or a limit was exceeded.
    Decode,
}

/// Every issue found in one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub kind: ReportKind,
    pub issues: Vec<Issue>,
    /// Whether collection stopped at the maximum number of errors.
    pub truncated: bool,
}
