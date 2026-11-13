//! Safe, borrowing readers of the views of a report. A view is valid while the [`Report`] it came
//! from lives and is not rewritten, which the lifetimes enforce.

use core::slice;

use crate::types::{
    OX_CHOICE_BOOL, OX_CHOICE_INTEGER, OX_CHOICE_NULL, OX_CHOICE_STRING, OX_CTX_BOOL, OX_CTX_FLOAT,
    OX_CTX_INTEGER, OX_CTX_INTEGERS, OX_CTX_TEXT, OX_CTX_TEXTS, OX_CTX_VALUES, OX_CTX_VARIANTS,
    OX_REPORT_DECODE, OX_SEGMENT_INDEX, OxChoiceValue, OxContextEntry, OxContextValue, OxIssue,
    OxPathSegment, OxReportView, OxStr, OxVariant,
};

/// The text of a string of a view.
///
/// # Safety
///
/// `text` must come from a view of a live report: valid UTF-8, readable for `'a`.
unsafe fn text<'a>(text: OxStr) -> &'a str {
    // SAFETY: the caller guarantees `text` is valid UTF-8 readable for `'a`; oxiform builds every
    // `OxStr` of a view from a `String` or a `&'static str` and keeps it until the report is freed.
    unsafe { core::str::from_utf8_unchecked(slice::from_raw_parts(text.ptr, text.len)) }
}

/// A slice of a view's array.
///
/// # Safety
///
/// `pointer` must be null (with `len == 0`) or point to `len` valid `T` readable for `'a`.
unsafe fn array<'a, T>(pointer: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        return &[];
    }
    // SAFETY: the caller guarantees `len` valid `T` are readable at `pointer` for `'a`.
    unsafe { slice::from_raw_parts(pointer, len) }
}

/// The issues of a failed decode.
#[derive(Debug, Clone, Copy)]
pub struct ReportView<'a> {
    pub(crate) raw: &'a OxReportView,
}

impl<'a> ReportView<'a> {
    /// The view over a raw `OxReportView`.
    ///
    /// # Safety
    ///
    /// `raw` must be a view returned by `report_view`, and the report it belongs to must outlive
    /// `'a` unchanged.
    pub unsafe fn from_raw(raw: &'a OxReportView) -> Self {
        Self { raw }
    }

    /// Whether reading stopped (malformed input, a limit) instead of the input not fitting the type.
    pub fn is_decode_error(&self) -> bool {
        self.raw.kind == OX_REPORT_DECODE
    }

    /// Whether collection stopped at `max_errors`.
    pub fn truncated(&self) -> bool {
        self.raw.truncated != 0
    }

    pub fn len(&self) -> usize {
        self.raw.issue_count
    }

    pub fn is_empty(&self) -> bool {
        self.raw.issue_count == 0
    }

    pub fn issues(&self) -> impl Iterator<Item = IssueView<'a>> + use<'a> {
        issues(self.raw.issues, self.raw.issue_count, self.raw.issue_size)
    }
}

fn issues<'a>(
    start: *const OxIssue,
    count: usize,
    stride: u32,
) -> impl Iterator<Item = IssueView<'a>> + use<'a> {
    (0..count).map(move |index| {
        // SAFETY: `start` and `stride` come from a view of a live report that has `count` issues
        // `stride` bytes apart, each valid for `'a`.
        let raw = unsafe {
            &*start
                .cast::<u8>()
                .add(index * stride as usize)
                .cast::<OxIssue>()
        };
        IssueView { raw, stride }
    })
}

/// One problem found in the input.
#[derive(Debug, Clone, Copy)]
pub struct IssueView<'a> {
    raw: &'a OxIssue,
    stride: u32,
}

/// One step of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment<'a> {
    Key(&'a str),
    Index(u64),
}

/// A value of `ctx`.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextValue<'a> {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(&'a str),
    Integers(&'a [i64]),
    Texts(Vec<&'a str>),
    Values(Vec<ChoiceValue<'a>>),
    /// The issues of each variant of a union that was tried, by variant name.
    Variants(Vec<(&'a str, Vec<IssueView<'a>>)>),
}

/// A value a `Literal` allows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChoiceValue<'a> {
    Null,
    Bool(bool),
    Integer(i64),
    String(&'a str),
}

impl PartialEq for IssueView<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.code() == other.code()
            && self.message() == other.message()
            && self.pointer() == other.pointer()
            && self.input() == other.input()
            && self.path().eq(other.path())
            && self.context().eq(other.context())
    }
}

impl<'a> IssueView<'a> {
    /// The stable code, such as `"too_short"`.
    pub fn code(&self) -> &'a str {
        // SAFETY: a string of a view of a live report.
        unsafe { text(self.raw.code) }
    }

    pub fn message(&self) -> &'a str {
        // SAFETY: as above.
        unsafe { text(self.raw.message) }
    }

    /// The path as a JSON Pointer (RFC 6901).
    pub fn pointer(&self) -> &'a str {
        // SAFETY: as above.
        unsafe { text(self.raw.pointer) }
    }

    /// The received value as text, truncated, when there is one.
    pub fn input(&self) -> Option<&'a str> {
        // SAFETY: as above.
        (self.raw.has_input != 0).then(|| unsafe { text(self.raw.input) })
    }

    pub fn path(&self) -> impl Iterator<Item = Segment<'a>> + use<'a> {
        // SAFETY: `path` points to `path_len` valid segments of a live report.
        let segments: &'a [OxPathSegment] = unsafe { array(self.raw.path, self.raw.path_len) };
        segments.iter().map(|segment| {
            if segment.kind == OX_SEGMENT_INDEX {
                Segment::Index(segment.index)
            } else {
                // SAFETY: a string of a view of a live report.
                Segment::Key(unsafe { text(segment.key) })
            }
        })
    }

    pub fn context(&self) -> impl Iterator<Item = (&'a str, ContextValue<'a>)> + use<'a> {
        // SAFETY: `context` points to `context_len` valid entries of a live report.
        let entries: &'a [OxContextEntry] =
            unsafe { array(self.raw.context, self.raw.context_len) };
        let stride = self.stride;
        entries.iter().map(move |entry| {
            // SAFETY: a string of a view of a live report.
            let key = unsafe { text(entry.key) };
            (key, context_value(&entry.value, stride))
        })
    }
}

fn context_value<'a>(value: &'a OxContextValue, stride: u32) -> ContextValue<'a> {
    // SAFETY (all arms): `items` and `len` describe an array of the type the `kind` says, of a
    // live report; strings are strings of a view.
    unsafe {
        match value.kind {
            OX_CTX_BOOL => ContextValue::Bool(value.integer != 0),
            OX_CTX_INTEGER => ContextValue::Integer(value.integer),
            OX_CTX_FLOAT => ContextValue::Float(value.float),
            OX_CTX_TEXT => ContextValue::Text(text(value.text)),
            OX_CTX_INTEGERS => ContextValue::Integers(array(value.items.cast::<i64>(), value.len)),
            OX_CTX_TEXTS => ContextValue::Texts(
                array(value.items.cast::<OxStr>(), value.len)
                    .iter()
                    .map(|item| text(*item))
                    .collect(),
            ),
            OX_CTX_VALUES => ContextValue::Values(
                array(value.items.cast::<OxChoiceValue>(), value.len)
                    .iter()
                    .map(|choice| match choice.kind {
                        OX_CHOICE_NULL => ChoiceValue::Null,
                        OX_CHOICE_BOOL => ChoiceValue::Bool(choice.integer != 0),
                        OX_CHOICE_INTEGER => ChoiceValue::Integer(choice.integer),
                        OX_CHOICE_STRING => ChoiceValue::String(text(choice.text)),
                        _ => ChoiceValue::Null,
                    })
                    .collect(),
            ),
            OX_CTX_VARIANTS => ContextValue::Variants(
                array(value.items.cast::<OxVariant>(), value.len)
                    .iter()
                    .map(|variant| {
                        (
                            text(variant.name),
                            issues(variant.issues, variant.issue_count, stride).collect(),
                        )
                    })
                    .collect(),
            ),
            // A kind of a later minor version: the host built for this one does not know it.
            _ => ContextValue::Text(""),
        }
    }
}
