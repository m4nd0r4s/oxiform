//! Reports as C views.
//!
//! An `OxReport` is a [`Handle`]: the engine's report plus the arrays of views that point into it.
//! The views are built once, when the handle is made or rewritten, and never touched again, so
//! the pointers they hold stay valid until the handle is freed: they point to the heap buffers of
//! the report's strings and vectors, which do not move when a vector's header does, and to
//! vectors of the handle itself.

use core::ffi::c_void;

use oxiform_abi::{
    OX_BUFFER, OX_CHOICE_BOOL, OX_CHOICE_INTEGER, OX_CHOICE_NULL, OX_CHOICE_STRING, OX_CTX_BOOL,
    OX_CTX_FLOAT, OX_CTX_INTEGER, OX_CTX_INTEGERS, OX_CTX_TEXT, OX_CTX_TEXTS, OX_CTX_VALUES,
    OX_CTX_VARIANTS, OX_INTERNAL, OX_INVALID, OX_INVALID_ARGUMENT, OX_OK, OX_PYTHON_ERROR,
    OX_REPORT_DECODE, OX_REPORT_VALIDATION, OX_SEGMENT_INDEX, OX_SEGMENT_KEY, OX_UNSUPPORTED,
    OxChoiceValue, OxContextEntry, OxContextValue, OxIssue, OxLimits, OxPathSegment, OxReport,
    OxReportPart, OxReportView, OxStr, OxVariant, read_prefixed,
};
use oxiform_core::description::ChoiceValue;
use oxiform_core::issues::{ContextValue, ErrorCode, Issue, PathSegment, Report, ReportKind};
use oxiform_core::limits::Limits;
use oxiform_core::options::Options;

use crate::guard;

/// The report behind an `OxReport` pointer.
struct Handle {
    report: Report,
    level: Level,
    view: OxReportView,
}

/// An array of issue views and what they point to.
struct Level {
    views: Vec<OxIssue>,
    _extras: Vec<Extras>,
}

/// What one issue view points to besides the report: arrays built for it.
struct Extras {
    _pointer: String,
    _path: Vec<OxPathSegment>,
    _entries: Vec<OxContextEntry>,
    _owned: Vec<Owned>,
}

/// Arrays a view points to, held only to keep them alive.
#[allow(dead_code)]
enum Owned {
    Strs(Vec<OxStr>),
    Choices(Vec<OxChoiceValue>),
    Variants(Vec<OxVariant>, Vec<Level>),
}

fn text(value: &str) -> OxStr {
    OxStr::from_str(value)
}

fn build_level(issues: &[Issue]) -> Level {
    let mut views = Vec::with_capacity(issues.len());
    let mut extras = Vec::with_capacity(issues.len());
    for issue in issues {
        let (view, extra) = build_issue(issue);
        views.push(view);
        extras.push(extra);
    }
    Level {
        views,
        _extras: extras,
    }
}

fn build_issue(issue: &Issue) -> (OxIssue, Extras) {
    let pointer = issue.pointer();
    let path: Vec<OxPathSegment> = issue
        .path
        .iter()
        .map(|segment| match segment {
            PathSegment::Key(key) => OxPathSegment {
                kind: OX_SEGMENT_KEY,
                reserved: 0,
                index: 0,
                key: text(key),
            },
            PathSegment::Index(index) => OxPathSegment {
                kind: OX_SEGMENT_INDEX,
                reserved: 0,
                index: *index as u64,
                key: OxStr::EMPTY,
            },
        })
        .collect();
    let mut owned = Vec::new();
    let entries: Vec<OxContextEntry> = issue
        .context
        .iter()
        .map(|(key, value)| OxContextEntry {
            key: text(key),
            value: build_value(value, &mut owned),
        })
        .collect();
    let view = OxIssue {
        code: text(issue.code.as_str()),
        message: text(&issue.message),
        pointer: text(&pointer),
        input: issue.input.as_deref().map_or(OxStr::EMPTY, text),
        has_input: u32::from(issue.input.is_some()),
        reserved: 0,
        path: path.as_ptr(),
        path_len: path.len(),
        context: entries.as_ptr(),
        context_len: entries.len(),
    };
    (
        view,
        Extras {
            _pointer: pointer,
            _path: path,
            _entries: entries,
            _owned: owned,
        },
    )
}

fn scalar(kind: u32) -> OxContextValue {
    OxContextValue {
        kind,
        reserved: 0,
        integer: 0,
        float: 0.0,
        text: OxStr::EMPTY,
        items: core::ptr::null(),
        len: 0,
    }
}

fn build_value(value: &ContextValue, owned: &mut Vec<Owned>) -> OxContextValue {
    match value {
        ContextValue::Bool(flag) => OxContextValue {
            integer: i64::from(*flag),
            ..scalar(OX_CTX_BOOL)
        },
        ContextValue::Integer(number) => OxContextValue {
            integer: *number,
            ..scalar(OX_CTX_INTEGER)
        },
        ContextValue::Float(number) => OxContextValue {
            float: *number,
            ..scalar(OX_CTX_FLOAT)
        },
        ContextValue::Text(string) => OxContextValue {
            text: text(string),
            ..scalar(OX_CTX_TEXT)
        },
        ContextValue::Integers(numbers) => OxContextValue {
            items: numbers.as_ptr().cast::<c_void>(),
            len: numbers.len(),
            ..scalar(OX_CTX_INTEGERS)
        },
        ContextValue::Texts(strings) => {
            let items: Vec<OxStr> = strings.iter().map(|string| text(string)).collect();
            let value = OxContextValue {
                items: items.as_ptr().cast::<c_void>(),
                len: items.len(),
                ..scalar(OX_CTX_TEXTS)
            };
            owned.push(Owned::Strs(items));
            value
        }
        ContextValue::Values(values) => {
            let items: Vec<OxChoiceValue> = values
                .iter()
                .map(|choice| {
                    let (kind, integer, string) = match choice {
                        ChoiceValue::Null => (OX_CHOICE_NULL, 0, OxStr::EMPTY),
                        ChoiceValue::Bool(flag) => (OX_CHOICE_BOOL, i64::from(*flag), OxStr::EMPTY),
                        ChoiceValue::Integer(number) => (OX_CHOICE_INTEGER, *number, OxStr::EMPTY),
                        ChoiceValue::String(string) => (OX_CHOICE_STRING, 0, text(string)),
                    };
                    OxChoiceValue {
                        kind,
                        reserved: 0,
                        integer,
                        text: string,
                    }
                })
                .collect();
            let value = OxContextValue {
                items: items.as_ptr().cast::<c_void>(),
                len: items.len(),
                ..scalar(OX_CTX_VALUES)
            };
            owned.push(Owned::Choices(items));
            value
        }
        ContextValue::Variants(variants) => {
            let levels: Vec<Level> = variants
                .iter()
                .map(|(_, issues)| build_level(issues))
                .collect();
            let items: Vec<OxVariant> = variants
                .iter()
                .zip(&levels)
                .map(|((name, _), level)| OxVariant {
                    name: text(name),
                    issues: level.views.as_ptr(),
                    issue_count: level.views.len(),
                })
                .collect();
            let value = OxContextValue {
                items: items.as_ptr().cast::<c_void>(),
                len: items.len(),
                ..scalar(OX_CTX_VARIANTS)
            };
            owned.push(Owned::Variants(items, levels));
            value
        }
    }
}

impl Handle {
    fn new(report: Report) -> Box<Self> {
        let mut handle = Box::new(Self {
            report,
            level: Level {
                views: Vec::new(),
                _extras: Vec::new(),
            },
            view: OxReportView {
                struct_size: size_of::<OxReportView>() as u32,
                kind: 0,
                truncated: 0,
                issue_size: size_of::<OxIssue>() as u32,
                issues: core::ptr::null(),
                issue_count: 0,
            },
        });
        handle.rebuild();
        handle
    }

    /// Builds the views again, after the report changed.
    fn rebuild(&mut self) {
        let level = build_level(&self.report.issues);
        self.view = OxReportView {
            struct_size: size_of::<OxReportView>() as u32,
            kind: match self.report.kind {
                ReportKind::Validation => OX_REPORT_VALIDATION,
                ReportKind::Decode => OX_REPORT_DECODE,
            },
            truncated: u32::from(self.report.truncated),
            issue_size: size_of::<OxIssue>() as u32,
            issues: level.views.as_ptr(),
            issue_count: level.views.len(),
        };
        self.level = level;
    }
}

/// Sets the `format` context of the issues that carry one in Python: the `syntax` issues.
fn name_the_format(issues: &mut [Issue], format: &'static str) {
    for issue in issues {
        if issue.code == ErrorCode::Syntax {
            issue
                .context
                .push(("format", ContextValue::Text(format.to_owned())));
        }
        for (_, value) in &mut issue.context {
            if let ContextValue::Variants(variants) = value {
                for (_, nested) in variants {
                    name_the_format(nested, format);
                }
            }
        }
    }
}

/// A report for a host: the pointer the host owns, to free with [`report_free`] (or to consume
/// with [`report_combine`]). `format` names the format that was read (`"json"`), as the `ctx` of
/// a `syntax` issue does in Python.
pub fn into_report_pointer(mut report: Report, format: &'static str) -> *mut OxReport {
    name_the_format(&mut report.issues, format);
    Box::into_raw(Handle::new(report)).cast::<OxReport>()
}

/// A sentence that says what a status code means and what to do.
///
/// # Safety
///
/// Any integer is allowed; the function has no requirements (it is `unsafe` only to fit the table).
pub unsafe extern "C" fn status_message(status: i32) -> OxStr {
    let message = match status {
        OX_OK => "ok",
        OX_INVALID => {
            "the input is invalid: read the issues of the report, and free it with report_free"
        }
        OX_PYTHON_ERROR => {
            "a Python exception is set in the calling thread: fetch it (PyErr_Fetch) and raise or report it"
        }
        OX_INVALID_ARGUMENT => {
            "an argument is wrong: check the pointers, sizes and values you pass"
        }
        OX_UNSUPPORTED => {
            "this oxiform does not support what the call asks for: it belongs to a later minor version of the ABI, so upgrade oxiform"
        }
        OX_BUFFER => {
            "the output buffer could not make room: check its reserve callback and its capacity"
        }
        OX_INTERNAL => {
            "an internal error of oxiform (a bug): please report it with the call that caused it"
        }
        _ => "an unknown status: this oxiform is older than the host, so upgrade oxiform",
    };
    text(message)
}

/// # Safety
///
/// `out` must be null (an error) or point to a writable `OxLimits` with `struct_size` set.
unsafe fn fill_limits(out: *mut OxLimits, limits: Limits) -> i32 {
    if out.is_null() {
        return OX_INVALID_ARGUMENT;
    }
    // SAFETY: the caller guarantees `out` is readable up to `struct_size`.
    let Ok(current) = (unsafe { read_prefixed(out) }) else {
        return OX_INVALID_ARGUMENT;
    };
    let value = OxLimits {
        max_bytes: limits.max_bytes as u64,
        max_depth: limits.max_depth as u64,
        max_items: limits.max_items as u64,
        max_str_len: limits.max_str_len as u64,
        max_int_digits: limits.max_int_digits as u64,
        ..current
    };
    // SAFETY: `out` is writable for `struct_size >= SIZE_V1_0` bytes, and only the fields of ABI
    // 1.0 are written, each inside `SIZE_V1_0`.
    unsafe {
        (&raw mut (*out).max_bytes).write(value.max_bytes);
        (&raw mut (*out).max_depth).write(value.max_depth);
        (&raw mut (*out).max_items).write(value.max_items);
        (&raw mut (*out).max_str_len).write(value.max_str_len);
        (&raw mut (*out).max_int_digits).write(value.max_int_digits);
    }
    OX_OK
}

/// Fills `*out` with the default limits.
///
/// # Safety
///
/// `out` must be null (an error) or point to a writable `OxLimits` with `struct_size` set.
pub unsafe extern "C" fn limits_default(out: *mut OxLimits) -> i32 {
    // SAFETY: forwarded from the caller.
    guard(|| unsafe { fill_limits(out, Options::default().limits) })
}

/// Fills `*out` with no size limits.
///
/// # Safety
///
/// As [`limits_default`].
pub unsafe extern "C" fn limits_none(out: *mut OxLimits) -> i32 {
    // SAFETY: forwarded from the caller.
    guard(|| unsafe { fill_limits(out, Limits::none()) })
}

/// The views of a report.
///
/// # Safety
///
/// `report` must be a live report of this oxiform, or null (which returns null).
pub unsafe extern "C" fn report_view(report: *const OxReport) -> *const OxReportView {
    if report.is_null() {
        return core::ptr::null();
    }
    // SAFETY: the caller guarantees `report` is a live handle made by `into_report_pointer`.
    unsafe { &raw const (*report.cast::<Handle>()).view }
}

/// Makes every issue of `report` an issue found in `source`.
///
/// # Safety
///
/// `report` must be a live report of this oxiform, not in use by another thread, and `source` a
/// string readable for its length.
pub unsafe extern "C" fn report_in_source(report: *mut OxReport, source: OxStr) -> i32 {
    guard(|| {
        if report.is_null() {
            return OX_INVALID_ARGUMENT;
        }
        // SAFETY: the caller guarantees `source` is readable for its length.
        let Some(source) = (unsafe { source_text(source) }) else {
            return OX_INVALID_ARGUMENT;
        };
        // SAFETY: the caller guarantees `report` is a live, unshared handle.
        let handle = unsafe { &mut *report.cast::<Handle>() };
        let report = std::mem::replace(
            &mut handle.report,
            Report {
                kind: ReportKind::Validation,
                issues: Vec::new(),
                truncated: false,
            },
        );
        handle.report = report.in_source(source);
        handle.rebuild();
        OX_OK
    })
}

/// A source: non-empty UTF-8.
///
/// # Safety
///
/// `source` must be readable for its length.
unsafe fn source_text<'a>(source: OxStr) -> Option<&'a str> {
    if source.len == 0 || source.ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `source.len` bytes are readable at `source.ptr`.
    let bytes = unsafe { core::slice::from_raw_parts(source.ptr, source.len) };
    core::str::from_utf8(bytes).ok()
}

/// One report for the parts of a request.
///
/// # Safety
///
/// `parts` must point to `count` `OxReportPart`, each with a live report of this oxiform and a
/// readable source; `out` must be writable.
pub unsafe extern "C" fn report_combine(
    parts: *const OxReportPart,
    count: usize,
    max_errors: u64,
    out: *mut *mut OxReport,
) -> i32 {
    guard(|| {
        if out.is_null() || (parts.is_null() && count > 0) {
            return OX_INVALID_ARGUMENT;
        }
        // SAFETY: the caller guarantees `out` is writable.
        unsafe { out.write(core::ptr::null_mut()) };
        let parts: &[OxReportPart] = if count == 0 {
            &[]
        } else {
            // SAFETY: the caller guarantees `count` parts are readable at `parts`.
            unsafe { core::slice::from_raw_parts(parts, count) }
        };
        let mut sources = Vec::with_capacity(count);
        for (index, part) in parts.iter().enumerate() {
            // SAFETY: the caller guarantees the sources are readable.
            let source = unsafe { source_text(part.source) };
            let duplicate = parts[..index]
                .iter()
                .any(|earlier| earlier.report == part.report);
            let Some(source) = source else {
                return OX_INVALID_ARGUMENT;
            };
            if part.report.is_null() || duplicate {
                return OX_INVALID_ARGUMENT;
            }
            sources.push(source);
        }
        // Everything is valid: from here on the call cannot fail, and consumes the reports.
        let reports = parts.iter().map(|part| {
            // SAFETY: a live handle, checked non-null and distinct from the others, which the
            // caller hands over: this takes ownership.
            let handle = unsafe { Box::from_raw(part.report.cast::<Handle>()) };
            handle.report
        });
        let combined = Report::combine(
            sources.into_iter().zip(reports),
            if max_errors == 0 {
                Options::DEFAULT_MAX_ERRORS
            } else {
                usize::try_from(max_errors).unwrap_or(usize::MAX)
            },
        );
        // SAFETY: `out` is writable.
        unsafe { out.write(Box::into_raw(Handle::new(combined)).cast::<OxReport>()) };
        OX_OK
    })
}

/// Frees a report.
///
/// # Safety
///
/// `report` must be null or a live report of this oxiform that is not used afterwards.
pub unsafe extern "C" fn report_free(report: *mut OxReport) {
    if report.is_null() {
        return;
    }
    // SAFETY: the caller guarantees `report` is a live handle it gives up.
    drop(unsafe { Box::from_raw(report.cast::<Handle>()) });
}
