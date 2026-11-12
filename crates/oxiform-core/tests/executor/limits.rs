use oxiform_core::description::{Constraints, TypeDescription};
use oxiform_core::issues::{ErrorCode, ReportKind};
use oxiform_core::limits::Limits;

use crate::support::{decode, decode_with, integer, plain, string};

fn list_of_integers() -> TypeDescription {
    TypeDescription::List {
        items: Box::new(integer()),
        constraints: Constraints::default(),
    }
}

#[test]
fn stops_at_the_maximum_number_of_errors() {
    let limits = Limits {
        max_errors: 2,
        ..Limits::default()
    };
    let report =
        decode_with(&plain(list_of_integers()), r#"["a", "b", "c"]"#, &limits).unwrap_err();
    assert_eq!(report.issues.len(), 2);
    assert!(report.truncated);
    assert_eq!(report.kind, ReportKind::Validation);
}

#[test]
fn reports_syntax_errors_as_decode_failures() {
    let report = decode(&plain(list_of_integers()), "[1, \"x\",]").unwrap_err();
    assert_eq!(report.kind, ReportKind::Decode);
    let codes: Vec<_> = report.issues.iter().map(|issue| issue.code).collect();
    assert_eq!(codes, [ErrorCode::Type, ErrorCode::Syntax]);
    assert_eq!(
        report.issues[1].message,
        "expected a value, found ']' (line 1, column 9)"
    );
}

#[test]
fn enforces_size_limits() {
    let cases = [
        (
            Limits {
                max_items: 2,
                ..Limits::default()
            },
            plain(list_of_integers()),
            "[1, 2, 3]",
        ),
        (
            Limits {
                max_str_len: 3,
                ..Limits::default()
            },
            plain(string()),
            "\"abcd\"",
        ),
        (
            Limits {
                max_int_digits: 3,
                ..Limits::default()
            },
            plain(integer()),
            "-1234",
        ),
        (
            Limits {
                max_depth: 2,
                ..Limits::default()
            },
            plain(TypeDescription::Any),
            "[[[1]]]",
        ),
    ];
    for (limits, description, input) in cases {
        let report = decode_with(&description, input, &limits).unwrap_err();
        assert_eq!(report.kind, ReportKind::Decode, "{input}");
        assert_eq!(
            report.issues.last().unwrap().code,
            ErrorCode::LimitExceeded,
            "{input}"
        );
    }
}

#[test]
fn deep_nesting_is_a_limit_error_even_with_no_limits() {
    for opening in ["[", r#"{"a":"#] {
        let closing = if opening == "[" { "]" } else { "}" };
        for depth in [Limits::DEPTH_CEILING + 1, 100_000] {
            let input = format!("{}1{}", opening.repeat(depth), closing.repeat(depth));
            let report =
                decode_with(&plain(TypeDescription::Any), &input, &Limits::none()).unwrap_err();
            let issue = report.issues.last().unwrap();
            assert_eq!(report.kind, ReportKind::Decode, "{depth}");
            assert_eq!(issue.code, ErrorCode::LimitExceeded, "{depth}");
            assert!(issue.message.contains("512"), "{}", issue.message);
        }
    }
    let ceiling = Limits::DEPTH_CEILING;
    let exactly = format!("{}1{}", "[".repeat(ceiling), "]".repeat(ceiling));
    assert!(decode_with(&plain(TypeDescription::Any), &exactly, &Limits::none()).is_ok());
}

#[test]
fn places_syntax_errors_at_the_key_being_read() {
    let report = decode(&plain(TypeDescription::Any), "{\"a\": {\"b\": }}").unwrap_err();
    assert_eq!(report.issues[0].pointer(), "/a/b");
}
