//! What the executor does with the validators of a sink: which values it hands over, when, and
//! how a rejection becomes an issue.

use oxiform_core::compiler::compile;
use oxiform_core::description::{Description, TypeDescription};
use oxiform_core::executor::{Failure, execute_pairs};
use oxiform_core::options::Options;
use oxiform_formats::pairs::PairsBuilder;

use crate::support::{
    Value, ValueSink, decode, decode_options, field, integer, issues, string, structure, summary,
};

fn validated(
    mut field: oxiform_core::description::FieldDescription,
) -> oxiform_core::description::FieldDescription {
    field.validated = true;
    field
}

fn with_default(
    mut field: oxiform_core::description::FieldDescription,
) -> oxiform_core::description::FieldDescription {
    field.has_default = true;
    field
}

/// `Account(first: int, second: str)`, with the checks of the test sink on what is marked.
fn account(first_validated: bool, second_validated: bool, model_validated: bool) -> Description {
    let mut first = field("first", integer());
    first.validated = first_validated;
    let mut second = field("second", string());
    second.validated = second_validated;
    let mut account = structure("Account", vec![first, second]);
    account.validated = model_validated;
    Description {
        root: TypeDescription::Struct(0),
        structs: vec![account],
        choices: Vec::new(),
        conversions: Vec::new(),
    }
}

#[test]
fn a_structure_without_validators_never_asks_the_sink() {
    // The test sink would reject -5 and "bad" if it were asked.
    let description = account(false, false, false);
    assert!(decode(&description, r#"{"first": -5, "second": "bad"}"#).is_ok());
}

#[test]
fn a_rejected_field_is_an_invalid_issue_at_the_field() {
    let description = account(true, false, false);
    let found = issues(&description, r#"{"first": -5, "second": "x"}"#);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code.as_str(), "invalid");
    assert_eq!(found[0].code_text(), "negative");
    assert_eq!(found[0].pointer(), "/first");
    assert_eq!(found[0].message, "-5 is negative");
    assert_eq!(found[0].input, None);
}

#[test]
fn a_rejection_path_is_relative_to_the_field() {
    let description = account(false, true, false);
    assert_eq!(
        summary(&description, r#"{"first": 1, "second": "bad"}"#),
        vec![("invalid", "/second/sub".to_owned(), "bad text".to_owned())]
    );
}

#[test]
fn a_replacement_is_what_the_instance_holds() {
    let description = account(true, false, false);
    assert_eq!(
        decode(&description, r#"{"first": 99, "second": "x"}"#),
        Ok(Value::Instance(
            0,
            vec![Some(Value::Integer(100)), Some(Value::String("x".into()))]
        ))
    );
}

#[test]
fn every_problem_is_reported_together_whatever_the_order_of_the_keys() {
    let description = account(true, true, false);
    let report = decode(&description, r#"{"second": "bad", "first": -1}"#).unwrap_err();
    // Validators run in field order once the map is read, not in the order of the keys.
    let pointers: Vec<_> = report.issues.iter().map(|issue| issue.pointer()).collect();
    assert_eq!(pointers, ["/first", "/second/sub"]);
}

#[test]
fn a_type_problem_does_not_hide_the_validator_problem_of_another_field() {
    let description = account(true, true, false);
    let found = summary(&description, r#"{"first": -1, "second": 5}"#);
    let codes: Vec<_> = found
        .iter()
        .map(|(code, pointer, _)| (*code, pointer.as_str()))
        .collect();
    // `second` has the wrong type, so its validator does not run; `first` still is checked.
    assert_eq!(codes, [("type", "/second"), ("invalid", "/first")]);
}

#[test]
fn a_field_the_input_did_not_give_is_not_validated() {
    let mut first = validated(with_default(field("first", integer())));
    first.has_default = true;
    let description = Description {
        root: TypeDescription::Struct(0),
        structs: vec![structure("Account", vec![first])],
        choices: Vec::new(),
        conversions: Vec::new(),
    };
    assert_eq!(
        decode(&description, "{}"),
        Ok(Value::Instance(0, vec![None]))
    );
}

#[test]
fn a_model_validator_runs_on_the_instance_and_reports_at_the_model() {
    let description = account(false, false, true);
    let found = summary(&description, r#"{"first": 13, "second": "x"}"#);
    assert_eq!(
        found,
        [
            ("invalid", "".to_owned(), "unlucky".to_owned()),
            ("invalid", "/second".to_owned(), "really unlucky".to_owned()),
        ]
    );
    assert!(decode(&description, r#"{"first": 14, "second": "x"}"#).is_ok());
}

#[test]
fn a_model_validator_does_not_run_when_a_field_failed() {
    let description = account(false, false, true);
    let found = summary(&description, r#"{"first": 13, "second": 5}"#);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "type");
}

#[test]
fn the_containers_around_a_model_add_their_steps_to_its_rejection() {
    let inner = {
        let mut inner = structure("Inner", vec![field("first", integer())]);
        inner.validated = true;
        inner
    };
    let outer = structure(
        "Outer",
        vec![field(
            "items",
            TypeDescription::List {
                items: Box::new(TypeDescription::Struct(1)),
                constraints: Default::default(),
            },
        )],
    );
    let description = Description {
        root: TypeDescription::Struct(0),
        structs: vec![outer, inner],
        choices: Vec::new(),
        conversions: Vec::new(),
    };
    let found = summary(
        &description,
        r#"{"items": [{"first": 1}, {"first": 13}, {"first": 2}]}"#,
    );
    let pointers: Vec<_> = found
        .iter()
        .map(|(_, pointer, _)| pointer.as_str())
        .collect();
    assert_eq!(pointers, ["/items/1", "/items/1/second"]);
}

#[test]
fn max_errors_counts_the_problems_of_validators() {
    let description = account(true, true, false);
    let options = Options {
        max_errors: 1,
        ..Options::default()
    };
    let report =
        decode_options(&description, r#"{"first": -1, "second": "bad"}"#, &options).unwrap_err();
    assert_eq!(report.issues.len(), 1);
    assert!(report.truncated);
}

#[test]
fn pairs_validate_what_a_name_gave_and_not_an_absent_optional_field() {
    let optional = TypeDescription::Optional(Box::new(integer()));
    let mut page = validated(field("page", integer()));
    page.has_default = false;
    let after = validated(field("after", optional));
    let description = Description {
        root: TypeDescription::Struct(0),
        structs: vec![structure("Params", vec![page, after])],
        choices: Vec::new(),
        conversions: Vec::new(),
    };
    let plan = compile(&description).expect("the description compiles");
    let read = |input: &str| {
        let options = Options::default();
        let mut builder = PairsBuilder::new(&options.limits, false);
        builder.add_urlencoded(input.as_bytes());
        let mut reader = builder.finish();
        match execute_pairs(&plan, &mut reader, &mut ValueSink, &options, false) {
            Ok(value) => Ok(value),
            Err(Failure::Invalid(report)) => Err(report),
            Err(Failure::Sink(never)) => match never {},
        }
    };
    // `after` was not given: it is `None` and was not handed to the validator.
    assert_eq!(
        read("page=3"),
        Ok(Value::Instance(
            0,
            vec![Some(Value::Integer(3)), Some(Value::None)]
        ))
    );
    let report = read("page=-3&after=99").unwrap_err();
    let found: Vec<_> = report
        .issues
        .iter()
        .map(|issue| (issue.code_text().to_owned(), issue.pointer()))
        .collect();
    assert_eq!(found, [("negative".to_owned(), "/page".to_owned())]);
    let replaced = read("page=3&after=99").unwrap();
    assert_eq!(
        replaced,
        Value::Instance(0, vec![Some(Value::Integer(3)), Some(Value::Integer(100))])
    );
}
