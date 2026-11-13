//! Safe, borrowing readers of the views of a report. A view is valid while the [`Report`] it came
//! from lives and is not rewritten, which the lifetimes enforce.

use core::slice;

use crate::types::{
    OX_CHOICE_BOOL, OX_CHOICE_INTEGER, OX_CHOICE_NULL, OX_CHOICE_STRING, OX_CTX_BOOL, OX_CTX_FLOAT,
    OX_CTX_INTEGER, OX_CTX_INTEGERS, OX_CTX_TEXT, OX_CTX_TEXTS, OX_CTX_VALUES, OX_CTX_VARIANTS,
    OX_REPORT_DECODE, OX_SEGMENT_INDEX, OX_SEGMENT_KEY, OxChoiceValue, OxContextValue, OxIssue,
    OxReportView, OxStr, OxVariant,
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
        let strides = Strides {
            issue: self.raw.issue_size,
            segment: self.raw.segment_size,
            entry: self.raw.entry_size,
            variant: self.raw.variant_size,
            choice: self.raw.choice_size,
        };
        issues(self.raw.issues, self.raw.issue_count, strides)
    }
}

/// The strides of the arrays of structs of a report.
#[derive(Debug, Clone, Copy)]
struct Strides {
    issue: u32,
    segment: u32,
    entry: u32,
    variant: u32,
    choice: u32,
}

/// The elements of an array of structs `stride` bytes apart.
///
/// # Safety
///
/// `start` must point to `count` elements `stride` bytes apart, each valid for `'a`: an array of a
/// live report. A `stride` shorter than `T` (a report of an oxiform older than this host, which the
/// version check at import rules out) yields nothing instead of reading out of bounds.
unsafe fn strided<'a, T: 'a>(
    start: *const T,
    count: usize,
    stride: u32,
) -> impl Iterator<Item = &'a T> + use<'a, T> {
    let usable = count > 0 && stride as usize >= size_of::<T>();
    (0..if usable { count } else { 0 }).map(move |index| {
        // SAFETY: the caller guarantees `count` elements `stride` bytes apart at `start`.
        unsafe { &*start.cast::<u8>().add(index * stride as usize).cast::<T>() }
    })
}

fn issues<'a>(
    start: *const OxIssue,
    count: usize,
    strides: Strides,
) -> impl Iterator<Item = IssueView<'a>> + use<'a> {
    // SAFETY: `start`, `count` and the stride come from a view of a live report.
    unsafe { strided(start, count, strides.issue) }.map(move |raw| IssueView { raw, strides })
}

/// One problem found in the input.
#[derive(Debug, Clone, Copy)]
pub struct IssueView<'a> {
    raw: &'a OxIssue,
    strides: Strides,
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

    /// The steps of the path. A step of a kind this host does not know is skipped.
    pub fn path(&self) -> impl Iterator<Item = Segment<'a>> + use<'a> {
        // SAFETY: `path` points to `path_len` segments of a live report.
        let segments = unsafe { strided(self.raw.path, self.raw.path_len, self.strides.segment) };
        segments.filter_map(|segment| match segment.kind {
            OX_SEGMENT_INDEX => Some(Segment::Index(segment.index)),
            // SAFETY: a string of a view of a live report.
            OX_SEGMENT_KEY => Some(Segment::Key(unsafe { text(segment.key) })),
            _ => None,
        })
    }

    /// The entries of `ctx`. An entry of a kind this host does not know is skipped.
    pub fn context(&self) -> impl Iterator<Item = (&'a str, ContextValue<'a>)> + use<'a> {
        let strides = self.strides;
        // SAFETY: `context` points to `context_len` entries of a live report.
        let entries = unsafe { strided(self.raw.context, self.raw.context_len, strides.entry) };
        entries.filter_map(move |entry| {
            // SAFETY: a string of a view of a live report.
            let key = unsafe { text(entry.key) };
            context_value(&entry.value, strides).map(|value| (key, value))
        })
    }
}

fn context_value(value: &OxContextValue, strides: Strides) -> Option<ContextValue<'_>> {
    // SAFETY (all arms): `items` and `len` describe an array of the type the `kind` says, of a
    // live report; strings are strings of a view.
    unsafe {
        Some(match value.kind {
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
                strided(
                    value.items.cast::<OxChoiceValue>(),
                    value.len,
                    strides.choice,
                )
                .filter_map(|choice| match choice.kind {
                    OX_CHOICE_NULL => Some(ChoiceValue::Null),
                    OX_CHOICE_BOOL => Some(ChoiceValue::Bool(choice.integer != 0)),
                    OX_CHOICE_INTEGER => Some(ChoiceValue::Integer(choice.integer)),
                    OX_CHOICE_STRING => Some(ChoiceValue::String(text(choice.text))),
                    _ => None,
                })
                .collect(),
            ),
            OX_CTX_VARIANTS => ContextValue::Variants(
                strided(value.items.cast::<OxVariant>(), value.len, strides.variant)
                    .map(|variant| {
                        (
                            text(variant.name),
                            issues(variant.issues, variant.issue_count, strides).collect(),
                        )
                    })
                    .collect(),
            ),
            // A kind of a later minor version: skipped.
            _ => return None,
        })
    }
}
