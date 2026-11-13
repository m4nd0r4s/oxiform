use oxiform_core::compiler::compile;
use oxiform_core::description::{Bound, Constraints, Description, TypeDescription};

use crate::support::{
    Value, decode, decode_lax, field, integer, issues, plain, string, structure, summary,
};

fn secret(inner: TypeDescription) -> TypeDescription {
    TypeDescription::Secret(Box::new(inner))
}

fn wrapped(value: Value) -> Value {
    Value::Secret(Box::new(value))
}

fn at_least_8() -> TypeDescription {
    TypeDescription::Integer(Constraints {
        minimum: Some(Bound::Integer(8)),
        ..Constraints::default()
    })
}

#[test]
fn wraps_what_the_inner_type_reads() {
    let description = plain(secret(string()));
    assert_eq!(
        decode(&description, r#""hunter2""#),
        Ok(wrapped(Value::String("hunter2".into())))
    );
}

#[test]
fn reads_lax_text_as_the_inner_type() {
    let description = plain(secret(integer()));
    assert_eq!(
        decode_lax(&description, r#""42""#),
        Ok(wrapped(Value::Integer(42)))
    );
}

#[test]
fn an_issue_of_a_secret_keeps_its_code_and_message_but_not_the_input() {
    let description = plain(secret(integer()));
    let found = issues(&description, r#""hunter2""#);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code.as_str(), "type");
    assert_eq!(found[0].message, "expected int, received str");
    assert_eq!(found[0].input, None);
    // The same mismatch outside a secret quotes the input.
    let open = issues(&plain(integer()), r#""hunter2""#);
    assert_eq!(open[0].input.as_deref(), Some("hunter2"));
}

#[test]
fn a_constraint_failure_inside_a_secret_has_no_input() {
    let description = plain(secret(at_least_8()));
    let found = issues(&description, "3");
    assert_eq!(found[0].code.as_str(), "too_small");
    assert_eq!(found[0].input, None);
}

#[test]
fn only_the_secret_is_redacted() {
    let description = Description {
        root: TypeDescription::Struct(0),
        structs: vec![structure(
            "Login",
            vec![
                field("user", integer()),
                field("password", secret(integer())),
            ],
        )],
        ..plain(TypeDescription::Struct(0))
    };
    let found = issues(&description, r#"{"user": "alice", "password": "hunter2"}"#);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].pointer(), "/user");
    assert_eq!(found[0].input.as_deref(), Some("alice"));
    assert_eq!(found[1].pointer(), "/password");
    assert_eq!(found[1].input, None);
}

#[test]
fn a_secret_inside_a_list_is_redacted_at_its_index() {
    let description = plain(TypeDescription::List {
        items: Box::new(secret(integer())),
        constraints: Constraints::default(),
    });
    assert_eq!(
        decode(&description, "[1, 2]"),
        Ok(Value::List(vec![
            wrapped(Value::Integer(1)),
            wrapped(Value::Integer(2))
        ]))
    );
    let found = issues(&description, r#"[1, "oops"]"#);
    assert_eq!(found[0].pointer(), "/1");
    assert_eq!(found[0].input, None);
}

#[test]
fn an_optional_secret_reads_null_and_the_empty_string_as_none() {
    let description = plain(TypeDescription::Optional(Box::new(secret(integer()))));
    assert_eq!(decode(&description, "null"), Ok(Value::None));
    assert_eq!(decode_lax(&description, r#""""#), Ok(Value::None));
    assert_eq!(
        decode_lax(&description, r#""7""#),
        Ok(wrapped(Value::Integer(7)))
    );
    // A secret string keeps the empty string, as `str | None` does.
    let text = plain(TypeDescription::Optional(Box::new(secret(string()))));
    assert_eq!(
        decode_lax(&text, r#""""#),
        Ok(wrapped(Value::String(String::new())))
    );
}

#[test]
fn a_secret_of_a_union_redacts_every_variant() {
    let description = plain(secret(TypeDescription::Union(vec![integer(), string()])));
    let found = issues(&description, "[1]");
    assert_eq!(found[0].input, None);
    assert_eq!(
        summary(&description, "true")[0].0,
        "type",
        "the code is kept"
    );
}

#[test]
fn a_secret_cannot_hold_a_secret() {
    let error = compile(&plain(secret(secret(string())))).expect_err("refused");
    assert!(error.message.contains("cannot hold a Secret"), "{error}");
}

#[test]
fn a_secret_cannot_be_one_of_several_variants() {
    let error = compile(&plain(TypeDescription::Union(vec![
        secret(string()),
        integer(),
    ])))
    .expect_err("refused");
    assert!(
        error
            .message
            .contains("Secret[str] cannot be a variant of a union"),
        "{error}"
    );
}
