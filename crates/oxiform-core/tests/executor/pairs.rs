use oxiform_core::compiler::compile;
use oxiform_core::description::{
    Constraints, Description, SequenceKind, TypeDescription, UnknownFields,
};
use oxiform_core::executor::{Failure, execute_pairs};
use oxiform_core::issues::{ErrorCode, Issue, Report, ReportKind};
use oxiform_core::limits::Limits;
use oxiform_core::options::Options;
use oxiform_formats::pairs::PairsBuilder;

use crate::support::{Value, ValueSink, field, integer, plain, string, structure};

fn optional(inner: TypeDescription) -> TypeDescription {
    TypeDescription::Optional(Box::new(inner))
}

fn list_of(items: TypeDescription) -> TypeDescription {
    TypeDescription::List {
        items: Box::new(items),
        constraints: Constraints::default(),
    }
}

fn collection(kind: SequenceKind, items: TypeDescription) -> TypeDescription {
    TypeDescription::Collection {
        kind,
        items: Box::new(items),
        constraints: Constraints::default(),
    }
}

/// `page: int`, `tags: list[str]`, `q: str | None = None`, `X-Request-Id` as `request_id`.
fn params() -> Description {
    let mut query = field("q", optional(string()));
    query.has_default = true;
    let mut tags = field("tags", list_of(string()));
    tags.has_default = true;
    let mut id = field("request_id", string());
    id.wire_name = "X-Request-Id".into();
    id.has_default = true;
    let mut page = field("page", integer());
    page.has_default = true;
    Description {
        root: TypeDescription::Struct(0),
        structs: vec![structure("Params", vec![page, tags, query, id])],
        choices: Vec::new(),
        conversions: Vec::new(),
    }
}

fn read_with(
    description: &Description,
    input: &str,
    options: &Options,
    fold_case: bool,
) -> Result<Value, Report> {
    let plan = compile(description).expect("the description compiles");
    let mut builder = PairsBuilder::new(&options.limits, fold_case);
    builder.add_urlencoded(input.as_bytes());
    let mut reader = builder.finish();
    match execute_pairs(&plan, &mut reader, &mut ValueSink, options, fold_case) {
        Ok(value) => Ok(value),
        Err(Failure::Invalid(report)) => Err(report),
        Err(Failure::Sink(never)) => match never {},
    }
}

fn read(description: &Description, input: &str) -> Result<Value, Report> {
    read_with(description, input, &Options::default(), false)
}

fn problems(description: &Description, input: &str) -> Vec<(&'static str, String)> {
    read(description, input)
        .expect_err("the input should be invalid")
        .issues
        .iter()
        .map(|issue: &Issue| (issue.code.as_str(), issue.pointer()))
        .collect()
}

fn s(text: &str) -> Value {
    Value::String(text.into())
}

#[test]
fn reads_each_field_from_its_value_as_lax_text() {
    assert_eq!(
        read(&params(), "page=3&tags=a&q=x"),
        Ok(Value::Instance(
            0,
            vec![
                Some(Value::Integer(3)),
                Some(Value::List(vec![s("a")])),
                Some(s("x")),
                None
            ]
        ))
    );
}

#[test]
fn a_repeated_key_is_a_list_in_order_and_a_single_one_is_a_list_of_one() {
    let Ok(Value::Instance(_, fields)) = read(&params(), "tags=b&page=1&tags=a&tags=c") else {
        panic!("should be valid")
    };
    assert_eq!(fields[1], Some(Value::List(vec![s("b"), s("a"), s("c")])));
}

#[test]
fn tuples_and_sets_read_the_values_too() {
    let mut description = params();
    description.structs[0].fields[1].value_type = collection(SequenceKind::Tuple, integer());
    let Ok(Value::Instance(_, fields)) = read(&description, "tags=1&tags=2") else {
        panic!("should be valid")
    };
    assert_eq!(
        fields[1],
        Some(Value::Tuple(vec![Value::Integer(1), Value::Integer(2)]))
    );
    description.structs[0].fields[1].value_type =
        optional(collection(SequenceKind::FrozenSet, integer()));
    let Ok(Value::Instance(_, fields)) = read(&description, "tags=1") else {
        panic!("should be valid")
    };
    assert_eq!(fields[1], Some(Value::Set(vec![Value::Integer(1)], true)));
}

#[test]
fn a_missing_optional_field_takes_its_default_and_a_missing_required_one_is_an_issue() {
    assert_eq!(
        read(&params(), ""),
        Ok(Value::Instance(0, vec![None, None, None, None]))
    );
    let mut description = params();
    description.structs[0].fields[0].has_default = false;
    assert_eq!(
        problems(&description, "tags=a"),
        [("missing", "/page".to_owned())]
    );
}

#[test]
fn an_empty_value_is_none_for_an_optional_field_and_text_for_a_str() {
    let mut description = params();
    description.structs[0].fields[0].value_type = optional(integer());
    let Ok(Value::Instance(_, fields)) = read(&description, "page=&q=") else {
        panic!("should be valid")
    };
    assert_eq!(fields[0], Some(Value::None));
    assert_eq!(fields[2], Some(s("")), "str | None keeps the empty string");
}

#[test]
fn a_repeated_key_on_a_scalar_field_is_a_duplicate_key() {
    assert_eq!(
        problems(&params(), "page=1&page=2&page=3"),
        [("duplicate_key", "/page".to_owned())]
    );
    let report = read(&params(), "q=a&q=b").unwrap_err();
    assert_eq!(report.issues[0].input.as_deref(), Some("q"));
    assert_eq!(report.kind, ReportKind::Validation);
}

#[test]
fn issues_point_at_the_key_and_the_index_of_the_value() {
    let mut description = params();
    description.structs[0].fields[1].value_type = list_of(integer());
    assert_eq!(
        problems(&description, "page=x&tags=1&tags=y&tags=3&tags=z"),
        [
            ("type", "/page".to_owned()),
            ("type", "/tags/1".to_owned()),
            ("type", "/tags/3".to_owned())
        ]
    );
}

#[test]
fn unknown_names_follow_the_policy_of_the_structure_or_of_the_call() {
    assert!(read(&params(), "other=1&page=1").is_ok());
    let forbid = Options {
        unknown: Some(UnknownFields::Forbid),
        ..Options::default()
    };
    let report = read_with(&params(), "page=1&other=1&other=2&x=1", &forbid, false).unwrap_err();
    let found: Vec<_> = report
        .issues
        .iter()
        .map(|issue| (issue.code, issue.pointer()))
        .collect();
    assert_eq!(
        found,
        [
            (ErrorCode::UnknownField, "/other".to_owned()),
            (ErrorCode::UnknownField, "/x".to_owned())
        ]
    );
    let mut description = params();
    description.structs[0].unknown_fields = UnknownFields::Forbid;
    assert!(read(&description, "other=1").is_err());
    let ignore = Options {
        unknown: Some(UnknownFields::Ignore),
        ..Options::default()
    };
    assert!(read_with(&description, "other=1", &ignore, false).is_ok());
}

#[test]
fn aliases_name_the_same_field_and_both_at_once_are_a_duplicate() {
    let mut description = params();
    description.structs[0].fields[0].aliases = vec!["p".into()];
    assert!(read(&description, "p=2").is_ok());
    assert_eq!(
        problems(&description, "p=2&page=3"),
        [("duplicate_key", "/page".to_owned())]
    );
}

#[test]
fn names_are_matched_without_regard_to_case_when_asked_and_only_then() {
    let options = Options::default();
    let folded = read_with(&params(), "x-request-id=abc&PAGE=4", &options, true);
    assert_eq!(
        folded,
        Ok(Value::Instance(
            0,
            vec![Some(Value::Integer(4)), None, None, Some(s("abc"))]
        ))
    );
    assert_eq!(
        read(&params(), "x-request-id=abc&PAGE=4"),
        Ok(Value::Instance(0, vec![None, None, None, None]))
    );
    let repeated = read_with(&params(), "Page=1&page=2", &options, true).unwrap_err();
    assert_eq!(repeated.issues[0].pointer(), "/page");
    assert_eq!(repeated.issues[0].code, ErrorCode::DuplicateKey);
}

#[test]
fn a_decode_fault_is_a_decode_report_with_the_key_in_the_pointer() {
    let report = read(&params(), "page=1&tags=ok&tags=%zz").unwrap_err();
    assert_eq!(report.kind, ReportKind::Decode);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].code, ErrorCode::Syntax);
    assert_eq!(report.issues[0].pointer(), "/tags");
    let in_name = read(&params(), "%zz=1").unwrap_err();
    assert_eq!(in_name.issues[0].pointer(), "");
}

#[test]
fn limits_apply_to_pairs_names_values_and_bytes() {
    let limits = |change: fn(&mut Limits)| {
        let mut options = Options::default();
        change(&mut options.limits);
        options
    };
    let report = read_with(
        &params(),
        "a=1&b=2&c=3",
        &limits(|l| l.max_items = 2),
        false,
    )
    .unwrap_err();
    assert_eq!(report.kind, ReportKind::Decode);
    assert_eq!(report.issues[0].code, ErrorCode::LimitExceeded);
    let report =
        read_with(&params(), "q=abcdef", &limits(|l| l.max_str_len = 5), false).unwrap_err();
    assert_eq!(report.issues[0].code, ErrorCode::LimitExceeded);
    let report = read_with(&params(), "q=abcdef", &limits(|l| l.max_bytes = 5), false).unwrap_err();
    assert_eq!(report.issues[0].code, ErrorCode::LimitExceeded);
}

#[test]
fn a_root_that_is_not_a_model_is_refused_with_what_to_declare() {
    let description = plain(string());
    let report = read(&description, "a=1").unwrap_err();
    assert_eq!(report.issues[0].code, ErrorCode::Type);
    assert!(report.issues[0].message.contains("class"));
}
