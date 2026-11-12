//! Prefixing issue paths with a source and combining the reports of several sources (OX-4).

use oxiform_core::issues::{ContextValue, ErrorCode, Issue, PathSegment, Report, ReportKind};

fn issue(code: ErrorCode, path: &[&str]) -> Issue {
    Issue {
        code,
        path: path
            .iter()
            .map(|key| PathSegment::Key((*key).to_owned()))
            .collect(),
        message: format!("{} problem", code.as_str()),
        input: Some("x".to_owned()),
        context: vec![("min", ContextValue::Integer(1))],
    }
}

fn report(kind: ReportKind, issues: Vec<Issue>, truncated: bool) -> Report {
    Report {
        kind,
        issues,
        truncated,
    }
}

fn validation(issues: Vec<Issue>) -> Report {
    report(ReportKind::Validation, issues, false)
}

fn pointers(report: &Report) -> Vec<String> {
    report.issues.iter().map(Issue::pointer).collect()
}

#[test]
fn a_source_becomes_the_first_segment_of_the_path() {
    let mut nested = issue(ErrorCode::TooShort, &["addresses"]);
    nested.path.push(PathSegment::Index(0));
    nested.path.push(PathSegment::Key("street".to_owned()));
    let prefixed = nested.in_source("body");
    assert_eq!(prefixed.pointer(), "/body/addresses/0/street");
    assert_eq!(prefixed.path[0], PathSegment::Key("body".to_owned()));
}

#[test]
fn an_issue_at_the_root_of_a_source_points_at_the_source() {
    assert_eq!(
        issue(ErrorCode::Type, &[]).in_source("query").pointer(),
        "/query"
    );
}

#[test]
fn prefixing_keeps_everything_but_the_path() {
    let before = issue(ErrorCode::TooSmall, &["page"]);
    let after = before.clone().in_source("query");
    assert_eq!(after.code, before.code);
    assert_eq!(after.message, before.message);
    assert_eq!(after.input, before.input);
    assert_eq!(after.context, before.context);
}

#[test]
fn prefixing_reaches_the_issues_of_union_variants() {
    let mut union = issue(ErrorCode::Union, &["value"]);
    union.context = vec![(
        "variants",
        ContextValue::Variants(vec![(
            "int".to_owned(),
            vec![issue(ErrorCode::Type, &["value"])],
        )]),
    )];
    let prefixed = union.in_source("header");
    let ContextValue::Variants(variants) = &prefixed.context[0].1 else {
        panic!("the variants are kept");
    };
    assert_eq!(variants[0].1[0].pointer(), "/header/value");
}

#[test]
fn a_source_with_a_slash_or_a_tilde_is_escaped_in_the_pointer() {
    assert_eq!(
        issue(ErrorCode::Type, &["a/b", "c~d"])
            .in_source("x/y~z")
            .pointer(),
        "/x~1y~0z/a~1b/c~0d"
    );
}

#[test]
fn three_sources_become_one_report_in_the_order_given() {
    let combined = Report::combine(
        [
            (
                "body",
                validation(vec![issue(ErrorCode::TooShort, &["name"])]),
            ),
            (
                "query",
                validation(vec![issue(ErrorCode::TooSmall, &["page"])]),
            ),
            ("path", validation(vec![issue(ErrorCode::Type, &["id"])])),
        ],
        100,
    );
    assert_eq!(combined.kind, ReportKind::Validation);
    assert!(!combined.truncated);
    assert_eq!(
        pointers(&combined),
        ["/body/name", "/query/page", "/path/id"]
    );
    let codes: Vec<_> = combined.issues.iter().map(|issue| issue.code).collect();
    assert_eq!(
        codes,
        [ErrorCode::TooShort, ErrorCode::TooSmall, ErrorCode::Type]
    );
}

#[test]
fn the_combined_report_keeps_at_most_max_errors_issues_and_says_it_stopped() {
    let part = |code| validation(vec![issue(code, &["a"]), issue(code, &["b"])]);
    let combined = Report::combine(
        [
            ("body", part(ErrorCode::Type)),
            ("query", part(ErrorCode::Missing)),
        ],
        3,
    );
    assert_eq!(pointers(&combined), ["/body/a", "/body/b", "/query/a"]);
    assert!(combined.truncated);
}

#[test]
fn exactly_max_errors_issues_are_not_truncated() {
    let part = validation(vec![
        issue(ErrorCode::Type, &["a"]),
        issue(ErrorCode::Type, &["b"]),
    ]);
    let combined = Report::combine([("body", part)], 2);
    assert_eq!(combined.issues.len(), 2);
    assert!(!combined.truncated);
}

#[test]
fn a_part_that_stopped_early_makes_the_combination_truncated() {
    let stopped = report(
        ReportKind::Validation,
        vec![issue(ErrorCode::Type, &["a"])],
        true,
    );
    let combined = Report::combine([("body", stopped), ("query", validation(vec![]))], 100);
    assert!(combined.truncated);
}

#[test]
fn the_combination_is_a_decode_report_only_when_every_part_is_one() {
    let unreadable = || {
        report(
            ReportKind::Decode,
            vec![issue(ErrorCode::Syntax, &[])],
            false,
        )
    };
    let both = Report::combine([("body", unreadable()), ("query", unreadable())], 100);
    assert_eq!(both.kind, ReportKind::Decode);
    let mixed = Report::combine(
        [
            ("body", unreadable()),
            ("query", validation(vec![issue(ErrorCode::Type, &["a"])])),
        ],
        100,
    );
    assert_eq!(mixed.kind, ReportKind::Validation);
    assert_eq!(mixed.issues[0].code, ErrorCode::Syntax);
    assert_eq!(Report::combine([], 100).kind, ReportKind::Validation);
}
