use oxiform_core::description::{Bound, Constraints, Literal, TypeDescription};
use oxiform_core::issues::ContextValue;

use crate::support::{Value, decode, integer, issues, plain, string, summary};

#[test]
fn reads_scalars() {
    assert_eq!(decode(&plain(integer()), "-42"), Ok(Value::Integer(-42)));
    assert_eq!(
        decode(
            &plain(TypeDescription::Float(Constraints::default())),
            "1.5e2"
        ),
        Ok(Value::Float(150.0))
    );
    assert_eq!(
        decode(&plain(TypeDescription::Float(Constraints::default())), "3"),
        Ok(Value::Float(3.0))
    );
    assert_eq!(
        decode(&plain(string()), "\"a\\nb\""),
        Ok(Value::String("a\nb".into()))
    );
    assert_eq!(
        decode(&plain(TypeDescription::Bool), "true"),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        decode(&plain(TypeDescription::None), "null"),
        Ok(Value::None)
    );
}

#[test]
fn keeps_integers_beyond_64_bits() {
    let digits = "-123456789012345678901234567890";
    assert_eq!(
        decode(&plain(integer()), digits),
        Ok(Value::BigInteger(digits.into()))
    );
}

#[test]
fn rejects_the_wrong_type_with_what_was_received() {
    let found = issues(&plain(integer()), "\"x\"");
    assert_eq!(found[0].message, "expected int, received str");
    assert_eq!(found[0].input.as_deref(), Some("x"));
    assert_eq!(
        found[0].context,
        [
            ("expected", ContextValue::Text("int".into())),
            ("received", ContextValue::Text("str".into()))
        ]
    );
    assert_eq!(
        summary(&plain(integer()), "4.5")[0].2,
        "expected int, received float"
    );
    assert_eq!(
        summary(&plain(integer()), "true")[0].2,
        "expected int, received bool"
    );
    assert_eq!(
        summary(&plain(TypeDescription::Bool), "1")[0].2,
        "expected bool, received int"
    );
    assert_eq!(
        summary(&plain(string()), "[1, 2]")[0].2,
        "expected str, received list"
    );
}

#[test]
fn rejects_floats_out_of_range() {
    let float = plain(TypeDescription::Float(Constraints::default()));
    assert_eq!(
        summary(&float, "1e400"),
        [(
            "type",
            String::new(),
            "number out of range for float".into()
        )]
    );
}

#[test]
fn checks_numeric_limits() {
    let bounded = plain(TypeDescription::Integer(Constraints {
        minimum: Some(Bound::Integer(0)),
        maximum: Some(Bound::Float(10.5)),
        ..Constraints::default()
    }));
    assert_eq!(decode(&bounded, "10"), Ok(Value::Integer(10)));
    assert_eq!(
        summary(&bounded, "-1"),
        [("too_small", String::new(), "must be at least 0".into())]
    );
    assert_eq!(
        summary(&bounded, "11"),
        [("too_large", String::new(), "must be at most 10.5".into())]
    );
    assert_eq!(
        summary(&bounded, "99999999999999999999999")[0].0,
        "too_large"
    );
    assert_eq!(
        summary(&bounded, "-99999999999999999999999")[0].0,
        "too_small"
    );
}

#[test]
fn checks_string_constraints() {
    let constrained = plain(TypeDescription::String(Constraints {
        min_length: Some(2),
        max_length: Some(4),
        pattern: Some("^[a-zé]+$".into()),
        ..Constraints::default()
    }));
    assert_eq!(
        decode(&constrained, "\"café\""),
        Ok(Value::String("café".into()))
    );
    assert_eq!(
        summary(&constrained, "\"a\""),
        [(
            "too_short",
            String::new(),
            "length 1 is less than the minimum 2".into()
        )]
    );
    assert_eq!(
        summary(&constrained, "\"abcde\""),
        [(
            "too_long",
            String::new(),
            "length 5 is more than the maximum 4".into()
        )]
    );
    assert_eq!(
        summary(&constrained, "\"AB\""),
        [(
            "pattern",
            String::new(),
            "does not match the pattern \"^[a-zé]+$\"".into()
        )]
    );
}

#[test]
fn checks_one_of() {
    let level = plain(TypeDescription::String(Constraints {
        one_of: Some(vec![
            Literal::String("basic".into()),
            Literal::String("admin".into()),
        ]),
        ..Constraints::default()
    }));
    assert_eq!(
        decode(&level, "\"admin\""),
        Ok(Value::String("admin".into()))
    );
    assert_eq!(
        summary(&level, "\"root\""),
        [(
            "literal",
            String::new(),
            "must be one of \"basic\", \"admin\"".into()
        )]
    );
    let code = plain(TypeDescription::Integer(Constraints {
        one_of: Some(vec![Literal::Integer(1), Literal::Integer(2)]),
        ..Constraints::default()
    }));
    assert_eq!(
        summary(&code, "3"),
        [("literal", String::new(), "must be one of 1, 2".into())]
    );
}

#[test]
fn reads_untyped_values() {
    let any = plain(TypeDescription::Any);
    assert_eq!(
        decode(&any, r#"{"a": [1, 2.5, "x", null, true]}"#),
        Ok(Value::Dict(vec![(
            Value::String("a".into()),
            Value::List(vec![
                Value::Integer(1),
                Value::Float(2.5),
                Value::String("x".into()),
                Value::None,
                Value::Bool(true),
            ]),
        )]))
    );
}

#[test]
fn reads_optionals() {
    let optional = plain(TypeDescription::Optional(Box::new(integer())));
    assert_eq!(decode(&optional, "null"), Ok(Value::None));
    assert_eq!(decode(&optional, "7"), Ok(Value::Integer(7)));
    assert_eq!(
        summary(&optional, "\"7\"")[0].2,
        "expected int, received str"
    );
}
