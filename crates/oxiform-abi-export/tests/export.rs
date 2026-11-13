//! The exporting half of the ABI, without Python: what a host sees through the table.

use oxiform_abi::*;
use oxiform_abi_export::*;
use oxiform_core::description::ChoiceValue;
use oxiform_core::issues::{
    ContextValue as Context, ErrorCode, Issue, PathSegment, Report as CoreReport, ReportKind,
};

fn issue(code: ErrorCode, path: Vec<PathSegment>, message: &str) -> Issue {
    Issue {
        code,
        path,
        message: message.to_owned(),
        input: None,
        context: Vec::new(),
    }
}

fn key(name: &str) -> PathSegment {
    PathSegment::Key(name.to_owned())
}

fn rich_report() -> CoreReport {
    let mut variant_issue = issue(ErrorCode::Type, vec![key("id")], "expected int");
    variant_issue.input = Some("\"x\"".into());
    let mut union = issue(
        ErrorCode::Union,
        vec![key("pet"), PathSegment::Index(2)],
        "no variant matched",
    );
    union.context = vec![(
        "variants",
        Context::Variants(vec![
            ("Cat".into(), vec![variant_issue]),
            (
                "Dog".into(),
                vec![issue(
                    ErrorCode::Missing,
                    vec![key("name")],
                    "field required",
                )],
            ),
        ]),
    )];
    let mut literal = issue(
        ErrorCode::Literal,
        vec![key("a/b"), key("~")],
        "not allowed",
    );
    literal.input = Some("7".into());
    literal.context = vec![
        ("flag", Context::Bool(true)),
        ("min", Context::Integer(-3)),
        ("max", Context::Float(2.5)),
        ("limit", Context::Text("max_bytes".into())),
        ("lengths", Context::Integers(vec![1, 2, 3])),
        ("names", Context::Texts(vec!["x".into(), "yz".into()])),
        (
            "allowed",
            Context::Values(vec![
                ChoiceValue::Null,
                ChoiceValue::Bool(false),
                ChoiceValue::Integer(4),
                ChoiceValue::String("s".into()),
            ]),
        ),
    ];
    CoreReport {
        kind: ReportKind::Validation,
        issues: vec![union, literal],
        truncated: false,
    }
}

fn read(report: *mut OxReport) -> (bool, bool, Vec<OwnedIssue>) {
    // SAFETY: `report` is a live report made by `into_report_pointer`, kept alive by the caller.
    let view = unsafe { ReportView::from_raw(&*report_view(report)) };
    (
        view.is_decode_error(),
        view.truncated(),
        view.issues().map(OwnedIssue::from).collect(),
    )
}

#[derive(Debug, PartialEq)]
struct OwnedIssue {
    code: String,
    message: String,
    pointer: String,
    input: Option<String>,
    path: Vec<String>,
    context: Vec<(String, String)>,
}

impl From<IssueView<'_>> for OwnedIssue {
    fn from(view: IssueView<'_>) -> Self {
        Self {
            code: view.code().into(),
            message: view.message().into(),
            pointer: view.pointer().into(),
            input: view.input().map(Into::into),
            path: view
                .path()
                .map(|segment| match segment {
                    Segment::Key(key) => format!("k:{key}"),
                    Segment::Index(index) => format!("i:{index}"),
                })
                .collect(),
            context: view
                .context()
                .map(|(key, value)| (key.to_owned(), render(&value)))
                .collect(),
        }
    }
}

fn render(value: &ContextValue<'_>) -> String {
    match value {
        ContextValue::Variants(variants) => variants
            .iter()
            .map(|(name, issues)| {
                let issues: Vec<_> = issues
                    .iter()
                    .map(|issue| OwnedIssue::from(*issue))
                    .collect();
                format!("{name}={issues:?}")
            })
            .collect::<Vec<_>>()
            .join(";"),
        other => format!("{other:?}"),
    }
}

#[test]
fn a_report_round_trips_every_part_of_an_issue() {
    let pointer = into_report_pointer(rich_report(), "json");
    let (decode, truncated, issues) = read(pointer);
    assert!(!decode && !truncated);
    assert_eq!(issues.len(), 2);
    let union = &issues[0];
    assert_eq!(union.code, "union");
    assert_eq!(union.pointer, "/pet/2");
    assert_eq!(union.path, ["k:pet", "i:2"]);
    assert!(
        union.context[0]
            .1
            .starts_with("Cat=[OwnedIssue { code: \"type\""),
        "{:?}",
        union.context
    );
    assert!(union.context[0].1.contains("Dog=["), "{:?}", union.context);
    let literal = &issues[1];
    assert_eq!(literal.pointer, "/a~1b/~0");
    assert_eq!(literal.path, ["k:a/b", "k:~"]);
    assert_eq!(literal.input.as_deref(), Some("7"));
    let context: Vec<_> = literal
        .context
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        context,
        [
            ("flag", "Bool(true)"),
            ("min", "Integer(-3)"),
            ("max", "Float(2.5)"),
            ("limit", "Text(\"max_bytes\")"),
            ("lengths", "Integers([1, 2, 3])"),
            ("names", "Texts([\"x\", \"yz\"])"),
            (
                "allowed",
                "Values([Null, Bool(false), Integer(4), String(\"s\")])"
            ),
        ]
    );
    // SAFETY: a live report that is not used afterwards.
    unsafe { report_free(pointer) };
}

#[test]
fn a_syntax_issue_names_the_format_like_python_does() {
    let mut syntax = issue(ErrorCode::Syntax, Vec::new(), "expected a value");
    syntax.context = vec![("line", Context::Integer(1))];
    let pointer = into_report_pointer(
        CoreReport {
            kind: ReportKind::Decode,
            issues: vec![syntax],
            truncated: true,
        },
        "json",
    );
    let (decode, truncated, issues) = read(pointer);
    assert!(decode && truncated);
    assert_eq!(issues[0].pointer, "");
    assert_eq!(
        issues[0].context,
        [
            ("line".into(), "Integer(1)".into()),
            ("format".into(), "Text(\"json\")".into())
        ]
    );
    // SAFETY: a live report that is not used afterwards.
    unsafe { report_free(pointer) };
}

#[test]
fn a_report_is_rewritten_in_a_source_and_its_view_is_rebuilt() {
    let pointer = into_report_pointer(rich_report(), "json");
    let (_, _, before) = read(pointer);
    // SAFETY: a live report; the source is readable.
    assert_eq!(
        unsafe { report_in_source(pointer, OxStr::from_str("query")) },
        OX_OK
    );
    let (_, _, after) = read(pointer);
    assert_eq!(after[0].pointer, "/query/pet/2");
    assert_eq!(after[0].path, ["k:query", "k:pet", "i:2"]);
    assert_eq!(after[1].pointer, "/query/a~1b/~0");
    assert_eq!(after[0].code, before[0].code);
    // The issues of the variants of a union move with it.
    assert!(
        after[0].context[0].1.contains("/query/id"),
        "{:?}",
        after[0].context
    );
    // SAFETY: a live report.
    assert_eq!(
        unsafe { report_in_source(pointer, OxStr::from_str("")) },
        OX_INVALID_ARGUMENT
    );
    // SAFETY: a live report.
    assert_eq!(
        unsafe {
            report_in_source(
                pointer,
                OxStr {
                    ptr: [0xff_u8].as_ptr(),
                    len: 1,
                },
            )
        },
        OX_INVALID_ARGUMENT
    );
    // SAFETY: a live report that is not used afterwards.
    unsafe { report_free(pointer) };
}

fn report_of(kind: ReportKind, count: usize, truncated: bool) -> *mut OxReport {
    into_report_pointer(
        CoreReport {
            kind,
            issues: (0..count)
                .map(|n| {
                    issue(
                        ErrorCode::Missing,
                        vec![key(&format!("f{n}"))],
                        "field required",
                    )
                })
                .collect(),
            truncated,
        },
        "json",
    )
}

#[test]
fn combined_reports_prefix_each_source_keep_the_budget_and_consume_their_parts() {
    let body = report_of(ReportKind::Decode, 1, false);
    let query = report_of(ReportKind::Validation, 3, false);
    let parts = [
        OxReportPart {
            source: OxStr::from_str("query"),
            report: query,
        },
        OxReportPart {
            source: OxStr::from_str("body"),
            report: body,
        },
    ];
    let mut combined = core::ptr::null_mut();
    // SAFETY: two live reports, readable sources, a writable out pointer.
    let status = unsafe { report_combine(parts.as_ptr(), parts.len(), 2, &raw mut combined) };
    assert_eq!(status, OX_OK);
    let (decode, truncated, issues) = read(combined);
    // A validation part next to a decode part is a validation report; the budget cut the body.
    assert!(!decode && truncated);
    let pointers: Vec<_> = issues.iter().map(|issue| issue.pointer.as_str()).collect();
    assert_eq!(pointers, ["/query/f0", "/query/f1"]);
    // SAFETY: a live report that is not used afterwards; the parts were consumed.
    unsafe { report_free(combined) };

    let only_decode = report_of(ReportKind::Decode, 1, false);
    let parts = [OxReportPart {
        source: OxStr::from_str("body"),
        report: only_decode,
    }];
    // SAFETY: as above; 0 means the default budget.
    assert_eq!(
        unsafe { report_combine(parts.as_ptr(), 1, 0, &raw mut combined) },
        OX_OK
    );
    assert!(read(combined).0);
    // SAFETY: a live report that is not used afterwards.
    unsafe { report_free(combined) };

    // No parts: an empty validation report.
    // SAFETY: no parts, a writable out pointer.
    assert_eq!(
        unsafe { report_combine(core::ptr::null(), 0, 0, &raw mut combined) },
        OX_OK
    );
    let (decode, truncated, issues) = read(combined);
    assert!(!decode && !truncated && issues.is_empty());
    // SAFETY: a live report that is not used afterwards.
    unsafe { report_free(combined) };
}

#[test]
fn a_combine_with_a_bad_argument_consumes_nothing() {
    let first = report_of(ReportKind::Validation, 1, false);
    let second = report_of(ReportKind::Validation, 1, false);
    let mut out = core::ptr::null_mut();
    let bad_sources = [
        OxReportPart {
            source: OxStr::from_str("body"),
            report: first,
        },
        OxReportPart {
            source: OxStr::from_str(""),
            report: second,
        },
    ];
    // SAFETY: live reports, readable sources, a writable out pointer.
    assert_eq!(
        unsafe { report_combine(bad_sources.as_ptr(), 2, 0, &raw mut out) },
        OX_INVALID_ARGUMENT
    );
    let twice = [
        OxReportPart {
            source: OxStr::from_str("a"),
            report: first,
        },
        OxReportPart {
            source: OxStr::from_str("b"),
            report: first,
        },
    ];
    // SAFETY: as above; the same report twice is refused before anything is taken.
    assert_eq!(
        unsafe { report_combine(twice.as_ptr(), 2, 0, &raw mut out) },
        OX_INVALID_ARGUMENT
    );
    let null_report = [OxReportPart {
        source: OxStr::from_str("a"),
        report: core::ptr::null_mut(),
    }];
    // SAFETY: as above.
    assert_eq!(
        unsafe { report_combine(null_report.as_ptr(), 1, 0, &raw mut out) },
        OX_INVALID_ARGUMENT
    );
    // SAFETY: as above.
    assert_eq!(
        unsafe { report_combine(null_report.as_ptr(), 1, 0, core::ptr::null_mut()) },
        OX_INVALID_ARGUMENT
    );
    assert!(out.is_null());
    // Both reports are still alive and readable.
    assert_eq!(read(first).2.len(), 1);
    assert_eq!(read(second).2.len(), 1);
    // SAFETY: live reports that are not used afterwards.
    unsafe {
        report_free(first);
        report_free(second);
    }
}

#[test]
fn null_reports_are_harmless() {
    // SAFETY: null is allowed by both.
    unsafe {
        report_free(core::ptr::null_mut());
        assert!(report_view(core::ptr::null()).is_null());
        assert_eq!(
            report_in_source(core::ptr::null_mut(), OxStr::from_str("x")),
            OX_INVALID_ARGUMENT
        );
    }
}

#[test]
fn every_status_has_a_message_that_says_what_to_do() {
    for status in [
        OX_OK,
        OX_INVALID,
        OX_PYTHON_ERROR,
        OX_INVALID_ARGUMENT,
        OX_UNSUPPORTED,
        OX_BUFFER,
        OX_INTERNAL,
        99,
    ] {
        // SAFETY: any integer is allowed; the string is static UTF-8.
        let message = unsafe {
            let text = status_message(status);
            core::str::from_utf8(core::slice::from_raw_parts(text.ptr, text.len)).unwrap()
        };
        assert!(!message.is_empty());
        if status != OX_OK {
            assert!(
                ["free", "fetch", "check", "upgrade", "report", "read"]
                    .iter()
                    .any(|verb| message.contains(verb)),
                "{status}: {message}"
            );
        }
    }
}

#[test]
fn limits_are_the_engines_and_a_longer_struct_is_filled_in_its_known_part() {
    let mut limits = OxLimits::zeroed();
    // SAFETY: a writable struct with `struct_size` set.
    assert_eq!(unsafe { limits_default(&raw mut limits) }, OX_OK);
    assert_eq!(
        (limits.max_bytes, limits.max_depth, limits.max_items),
        (64 << 20, 128, 1_000_000)
    );
    assert_eq!(
        (limits.max_str_len, limits.max_int_digits),
        (16 << 20, 4300)
    );
    // SAFETY: as above.
    assert_eq!(unsafe { limits_none(&raw mut limits) }, OX_OK);
    assert_eq!((limits.max_bytes, limits.max_depth), (u64::MAX, 128));
    // SAFETY: null is refused.
    assert_eq!(
        unsafe { limits_none(core::ptr::null_mut()) },
        OX_INVALID_ARGUMENT
    );
    limits.struct_size = 4;
    // SAFETY: only the first four bytes are read before the size check.
    assert_eq!(unsafe { limits_none(&raw mut limits) }, OX_INVALID_ARGUMENT);
}

#[test]
fn decode_options_cover_every_field() {
    // SAFETY (all calls): the options and limits are valid structs that live through the call.
    let defaults = unsafe { decode_options(core::ptr::null()) }.unwrap();
    assert_eq!(defaults, oxiform_core::options::Options::default());
    let mut raw = OxDecodeOptions::new();
    raw.mode = OX_MODE_LAX;
    raw.max_errors = 7;
    raw.unknown = OX_UNKNOWN_FORBID;
    let mut limits = OxLimits::zeroed();
    limits.max_bytes = 10;
    limits.max_depth = 1000;
    limits.max_items = 3;
    limits.max_str_len = 4;
    limits.max_int_digits = 5;
    raw.limits = &raw const limits;
    let read = unsafe { decode_options(&raw const raw) }.unwrap();
    assert_eq!(read.mode, oxiform_core::options::Mode::Lax);
    assert_eq!(read.max_errors, 7);
    assert_eq!(
        read.unknown,
        Some(oxiform_core::description::UnknownFields::Forbid)
    );
    assert_eq!(
        (
            read.limits.max_bytes,
            read.limits.max_depth,
            read.limits.max_items
        ),
        (10, 128, 3)
    );
    assert_eq!(
        (read.limits.max_str_len, read.limits.max_int_digits),
        (4, 5)
    );
    raw.unknown = OX_UNKNOWN_IGNORE;
    raw.limits = core::ptr::null();
    raw.max_errors = 0;
    let read = unsafe { decode_options(&raw const raw) }.unwrap();
    assert_eq!(
        read.unknown,
        Some(oxiform_core::description::UnknownFields::Ignore)
    );
    assert_eq!(read.max_errors, 100);
    assert_eq!(read.limits, oxiform_core::limits::Limits::DEFAULT);
}

#[test]
fn decode_options_refuse_what_they_cannot_honour_and_say_what_to_do() {
    let refused = |change: &dyn Fn(&mut OxDecodeOptions)| {
        let mut raw = OxDecodeOptions::new();
        change(&mut raw);
        // SAFETY: a valid struct that lives through the call.
        unsafe { decode_options(&raw const raw) }.unwrap_err()
    };
    let error = refused(&|raw| raw.mode = 9);
    assert_eq!(
        (error.status, error.message.contains("OX_MODE_LAX")),
        (OX_INVALID_ARGUMENT, true)
    );
    let error = refused(&|raw| raw.unknown = 9);
    assert_eq!(
        (error.status, error.message.contains("OX_UNKNOWN_FORBID")),
        (OX_INVALID_ARGUMENT, true)
    );
    let error = refused(&|raw| raw.flags = 0x10);
    assert_eq!(
        (error.status, error.message.contains("upgrade oxiform")),
        (OX_UNSUPPORTED, true)
    );
    let error = refused(&|raw| raw.context = core::ptr::without_provenance_mut(8));
    assert_eq!(
        (error.status, error.message.contains("pass NULL")),
        (OX_UNSUPPORTED, true)
    );
    let error = refused(&|raw| raw.struct_size = 8);
    assert_eq!(
        (error.status, error.message.contains("struct_size")),
        (OX_INVALID_ARGUMENT, true)
    );
    let mut raw = OxDecodeOptions::new();
    let mut limits = OxLimits::zeroed();
    limits.struct_size = 4;
    raw.limits = &raw const limits;
    // SAFETY: valid structs that live through the call.
    let error = unsafe { decode_options(&raw const raw) }.unwrap_err();
    assert!(
        error.message.starts_with("limits.struct_size is 4"),
        "{}",
        error.message
    );
}

#[test]
fn a_caller_built_against_a_later_minor_is_read_up_to_the_part_this_oxiform_knows() {
    #[repr(C)]
    struct Later {
        known: OxDecodeOptions,
        added: [u64; 3],
    }
    let mut later = Later {
        known: OxDecodeOptions::new(),
        added: [u64::MAX; 3],
    };
    later.known.struct_size = size_of::<Later>() as u32;
    later.known.mode = OX_MODE_LAX;
    // SAFETY: a valid struct of `struct_size` bytes.
    let read = unsafe { decode_options(&raw const later.known) }.unwrap();
    assert_eq!(read.mode, oxiform_core::options::Mode::Lax);
    assert_eq!(later.added[2], u64::MAX);
}

#[test]
fn encode_options_cover_every_flag() {
    // SAFETY (all calls): valid structs that live through the call.
    assert_eq!(
        unsafe { encode_options(core::ptr::null()) }.unwrap(),
        EncodeParams::default()
    );
    let mut raw = OxEncodeOptions::new();
    raw.flags = OX_ENCODE_SORT_KEYS | OX_ENCODE_INDENT;
    raw.indent = 4;
    assert_eq!(
        unsafe { encode_options(&raw const raw) }.unwrap(),
        EncodeParams {
            sort_keys: true,
            indent: Some(4)
        }
    );
    raw.flags = OX_ENCODE_INDENT;
    raw.indent = 0;
    assert_eq!(
        unsafe { encode_options(&raw const raw) }.unwrap().indent,
        Some(0)
    );
    raw.flags = 0x80;
    let error = unsafe { encode_options(&raw const raw) }.unwrap_err();
    assert_eq!(error.status, OX_UNSUPPORTED);
    raw.flags = 0;
    raw.struct_size = 1;
    assert_eq!(
        unsafe { encode_options(&raw const raw) }
            .unwrap_err()
            .status,
        OX_INVALID_ARGUMENT
    );
}

#[test]
fn appending_fills_the_spare_capacity_and_grows_through_reserve() {
    let mut buffer = VecBuffer::from_vec(b"[".to_vec());
    for chunk in [&b"1,"[..], b"2,", &[b'x'; 5000][..]] {
        // SAFETY: a buffer from `with_raw`, which keeps the contract.
        buffer.with_raw(|raw| unsafe { append(raw, chunk) }.unwrap());
    }
    assert_eq!(buffer.as_slice().len(), 1 + 2 + 2 + 5000);
    assert!(buffer.as_slice().starts_with(b"[1,2,xxx"));
}

fn fixed(storage: &mut [u8], len: usize) -> OxBuffer {
    OxBuffer {
        struct_size: size_of::<OxBuffer>() as u32,
        reserved: 0,
        data: storage.as_mut_ptr(),
        len,
        capacity: storage.len(),
        reserve: None,
        user_data: core::ptr::null_mut(),
    }
}

#[test]
fn a_fixed_buffer_that_is_too_small_is_left_alone_and_the_error_says_what_to_do() {
    let mut storage = *b"abcdefgh";
    let mut buffer = fixed(&mut storage, 6);
    // SAFETY: a valid buffer over `storage`.
    unsafe { append(&raw mut buffer, b"XY") }.unwrap();
    assert_eq!((buffer.len, &storage), (8, b"abcdefXY"));
    let mut storage = *b"abcdefgh";
    let mut buffer = fixed(&mut storage, 7);
    // SAFETY: as above.
    let error = unsafe { append(&raw mut buffer, b"XY") }.unwrap_err();
    assert_eq!(error.status, OX_BUFFER);
    assert!(
        error.message.contains("fixed at 8 bytes with 7 used") && error.message.contains("reserve"),
        "{}",
        error.message
    );
    assert_eq!((buffer.len, &storage), (7, b"abcdefgh"));
}

unsafe extern "C" fn refuse(_: *mut OxBuffer, _: usize) -> i32 {
    7
}

unsafe extern "C" fn lie_about_capacity(buffer: *mut OxBuffer, _: usize) -> i32 {
    // SAFETY: the buffer oxiform passes is valid.
    unsafe { (*buffer).capacity += 1 };
    0
}

unsafe extern "C" fn move_len(buffer: *mut OxBuffer, min: usize) -> i32 {
    // SAFETY: the buffer oxiform passes is valid.
    unsafe {
        (*buffer).capacity = min;
        (*buffer).len = 0;
    }
    0
}

#[test]
fn a_reserve_that_fails_or_breaks_its_contract_is_a_buffer_error_and_nothing_is_written() {
    for reserve in [refuse as OxReserveFn, lie_about_capacity, move_len] {
        let mut storage = [0_u8; 4];
        let mut buffer = fixed(&mut storage, 4);
        buffer.reserve = Some(reserve);
        // SAFETY: a valid buffer over `storage`; the callbacks only change fields.
        let error = unsafe { append(&raw mut buffer, b"more") }.unwrap_err();
        assert_eq!(error.status, OX_BUFFER, "{}", error.message);
        assert_eq!(storage, [0; 4]);
    }
}

#[test]
fn a_bad_buffer_is_an_invalid_argument() {
    // SAFETY: null is refused before anything is read.
    assert_eq!(
        unsafe { append(core::ptr::null_mut(), b"x") }
            .unwrap_err()
            .status,
        OX_INVALID_ARGUMENT
    );
    let mut storage = [0_u8; 4];
    let mut buffer = fixed(&mut storage, 0);
    buffer.struct_size = 8;
    // SAFETY: only `struct_size` is read before the size check.
    let error = unsafe { append(&raw mut buffer, b"x") }.unwrap_err();
    assert!(error.message.contains("struct_size"), "{}", error.message);
    buffer.struct_size = size_of::<OxBuffer>() as u32;
    buffer.len = 9;
    // SAFETY: a valid buffer struct.
    assert_eq!(
        unsafe { append(&raw mut buffer, b"x") }.unwrap_err().status,
        OX_INVALID_ARGUMENT
    );
    // Nothing to write is fine even with no data.
    buffer.len = 0;
    buffer.capacity = 0;
    buffer.data = core::ptr::null_mut();
    // SAFETY: as above.
    assert!(unsafe { append(&raw mut buffer, b"") }.is_ok());
}

#[test]
fn a_panic_is_returned_as_an_internal_error() {
    assert_eq!(guard(|| panic!("a bug")), OX_INTERNAL);
    assert_eq!(guard(|| OX_OK), OX_OK);
}

#[test]
fn a_zero_in_a_limit_is_the_default_of_that_field() {
    use oxiform_core::limits::Limits;
    // A host fills in one limit of a zeroed struct: the others are their defaults, not 0.
    let mut limits = OxLimits::zeroed();
    limits.max_bytes = 100;
    let mut raw = OxDecodeOptions::new();
    raw.limits = &raw const limits;
    // SAFETY: valid structs that live through the call.
    let read = unsafe { decode_options(&raw const raw) }.unwrap();
    assert_eq!(read.limits.max_bytes, 100);
    assert_eq!(
        (
            read.limits.max_depth,
            read.limits.max_items,
            read.limits.max_str_len,
            read.limits.max_int_digits
        ),
        (
            Limits::DEFAULT.max_depth,
            Limits::DEFAULT.max_items,
            Limits::DEFAULT.max_str_len,
            Limits::DEFAULT.max_int_digits
        )
    );
    // All zero is all defaults; u64::MAX removes a limit.
    let all_zero = OxLimits::zeroed();
    raw.limits = &raw const all_zero;
    // SAFETY: as above.
    assert_eq!(
        unsafe { decode_options(&raw const raw) }.unwrap().limits,
        Limits::DEFAULT
    );
    let mut none = OxLimits::zeroed();
    none.max_items = u64::MAX;
    raw.limits = &raw const none;
    // SAFETY: as above.
    assert_eq!(
        unsafe { decode_options(&raw const raw) }
            .unwrap()
            .limits
            .max_items,
        usize::MAX
    );
}

#[test]
fn a_bad_buffer_is_checked_before_any_work() {
    let mut storage = [0_u8; 4];
    let mut buffer = fixed(&mut storage, 5);
    // SAFETY: valid struct; `len > capacity` is what is being checked.
    assert_eq!(
        unsafe { check_buffer(&raw mut buffer) }.unwrap_err().status,
        OX_INVALID_ARGUMENT
    );
    buffer.len = 2;
    // SAFETY: as above.
    assert!(unsafe { check_buffer(&raw mut buffer) }.is_ok());
    // SAFETY: null is refused.
    assert!(unsafe { check_buffer(core::ptr::null_mut()) }.is_err());
}
