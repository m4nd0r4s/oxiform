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
    /// A validator rejected the value (`ox.Invalid`, `ValueError`, `TypeError`).
    Invalid,
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
            Self::Invalid => "invalid",
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
    /// The code a validator chose (`ox.Invalid(code="reserved")`): when it is set, `code` is
    /// [`ErrorCode::Invalid`] and this is what the issue reports. Read it with
    /// [`Issue::code_text`].
    pub user_code: Option<String>,
    pub path: Vec<PathSegment>,
    pub message: String,
    /// The received value as text, truncated; `None` for containers.
    pub input: Option<String>,
    pub context: Vec<(&'static str, ContextValue)>,
}

impl Issue {
    /// The code as text: the one a validator chose, or the stable code of `code`.
    pub fn code_text(&self) -> &str {
        self.user_code
            .as_deref()
            .unwrap_or_else(|| self.code.as_str())
    }

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

impl Issue {
    /// This issue as found inside `source` (`"body"`, `"query"`, ...): the source becomes the
    /// first segment of the path, and so of the pointer. The issues of union variants carried in
    /// the context move with it.
    pub fn in_source(mut self, source: &str) -> Self {
        self.path.insert(0, PathSegment::Key(source.to_owned()));
        for (_, value) in &mut self.context {
            if let ContextValue::Variants(variants) = value {
                for (_, issues) in variants {
                    let moved = std::mem::take(issues);
                    *issues = moved
                        .into_iter()
                        .map(|issue| issue.in_source(source))
                        .collect();
                }
            }
        }
        self
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

impl Report {
    /// This report for input that came from `source`: every path starts with it.
    pub fn in_source(mut self, source: &str) -> Self {
        self.issues = self
            .issues
            .into_iter()
            .map(|issue| issue.in_source(source))
            .collect();
        self
    }

    /// One report for the input of several sources, such as the body, the query string and the
    /// path of a request: each part is `(source, report)`.
    ///
    /// Issues keep the order of the parts and their own order inside a part, and every code,
    /// message and context is kept. At most `max_errors` issues are kept, the first ones, and
    /// the result is `truncated` when an issue was dropped or any part was already truncated.
    /// It is a decode report only when every part is one: a body that could not be read next to
    /// a query string that was read and rejected is still a validation report, whose issues
    /// carry the `syntax` and `limit_exceeded` codes of the part that could not be read.
    pub fn combine<'a>(
        parts: impl IntoIterator<Item = (&'a str, Report)>,
        max_errors: usize,
    ) -> Report {
        let mut issues = Vec::new();
        let mut truncated = false;
        let mut every_part_is_a_decode_report = true;
        let mut any_part = false;
        for (source, report) in parts {
            any_part = true;
            every_part_is_a_decode_report &= report.kind == ReportKind::Decode;
            truncated |= report.truncated;
            for issue in report.issues {
                if issues.len() < max_errors {
                    issues.push(issue.in_source(source));
                } else {
                    truncated = true;
                }
            }
        }
        Report {
            kind: if any_part && every_part_is_a_decode_report {
                ReportKind::Decode
            } else {
                ReportKind::Validation
            },
            issues,
            truncated,
        }
    }
}
