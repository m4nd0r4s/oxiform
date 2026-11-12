//! Lax mode: text is read as the type the field asks for (section 4.5 of the spec).

use oxiform_core::description::{
    Bound, ChoiceDescription, ChoiceValue, Constraints, ConversionDescription, ConversionInput,
    Description, SequenceKind, TypeDescription,
};
use oxiform_core::issues::ReportKind;

use crate::support::{Value, decode, decode_lax, field, integer, issues, plain, string, structure};

fn float() -> TypeDescription {
    TypeDescription::Float(Constraints::default())
}

fn optional(inner: TypeDescription) -> TypeDescription {
    TypeDescription::Optional(Box::new(inner))
}

fn conversion(input: ConversionInput) -> Description {
    Description {
        conversions: vec![ConversionDescription {
            name: "Thing".into(),
            format: "thing".into(),
            input,
        }],
        ..plain(TypeDescription::Converted(0))
    }
}

fn choice(values: Vec<ChoiceValue>) -> Description {
    Description {
        choices: vec![ChoiceDescription {
            name: "Choice".into(),
            values,
            is_enum: false,
        }],
        ..plain(TypeDescription::Choice(0))
    }
}

#[test]
fn reads_integers_from_text() {
    let int = plain(integer());
    for (input, value) in [
        (r#""42""#, 42),
        (r#""-7""#, -7),
        (r#""+3""#, 3),
        (r#""007""#, 7),
        (r#""42.0""#, 42),
        ("42.0", 42),
    ] {
        assert_eq!(
            decode_lax(&int, input),
            Ok(Value::Integer(value)),
            "{input}"
        );
    }
    assert_eq!(
        decode_lax(&int, r#""123456789012345678901234567890""#),
        Ok(Value::BigInteger("123456789012345678901234567890".into()))
    );
}

#[test]
fn never_reads_an_integer_with_a_fraction_or_a_bool() {
    let int = plain(integer());
    for input in [
        r#""4.5""#, "4.5", r#""1e3""#, r#"" 1""#, r#""one""#, r#""""#, "true", "null",
    ] {
        let found = issues(&int, input);
        assert_eq!(found.len(), 1, "{input}");
        let lax = decode_lax(&int, input).expect_err(input);
        assert_eq!(lax.issues[0].code.as_str(), "type", "{input}");
    }
}

#[test]
fn keeps_strict_mode_strict() {
    for (description, input) in [
        (plain(integer()), r#""42""#),
        (plain(integer()), "42.0"),
        (plain(float()), r#""1.5""#),
        (plain(TypeDescription::Bool), r#""true""#),
        (plain(optional(integer())), r#""""#),
        (choice(vec![ChoiceValue::Integer(1)]), r#""1""#),
    ] {
        assert!(decode(&description, input).is_err(), "{input}");
    }
}

#[test]
fn checks_constraints_after_coercion() {
    let bounded = plain(TypeDescription::Integer(Constraints {
        minimum: Some(Bound::Integer(1)),
        maximum: Some(Bound::Integer(10)),
        ..Constraints::default()
    }));
    assert_eq!(decode_lax(&bounded, r#""5""#), Ok(Value::Integer(5)));
    let report = decode_lax(&bounded, r#""50""#).unwrap_err();
    assert_eq!(report.issues[0].code.as_str(), "too_large");
    assert_eq!(report.issues[0].input.as_deref(), Some("50"));
}

#[test]
fn reads_floats_from_text() {
    let float = plain(float());
    for (input, value) in [
        (r#""1.5""#, 1.5),
        (r#""-.5""#, -0.5),
        (r#""2e2""#, 200.0),
        (r#""3""#, 3.0),
    ] {
        assert_eq!(
            decode_lax(&float, input),
            Ok(Value::Float(value)),
            "{input}"
        );
    }
    for input in [r#""inf""#, r#""nan""#, r#""1,5""#, r#""""#, "true"] {
        assert!(decode_lax(&float, input).is_err(), "{input}");
    }
    assert!(decode_lax(&float, r#""1e400""#).is_err());
}

#[test]
fn reads_booleans_from_words() {
    let boolean = plain(TypeDescription::Bool);
    for word in ["true", "True", "YES", "on", "1"] {
        assert_eq!(
            decode_lax(&boolean, &format!("\"{word}\"")),
            Ok(Value::Bool(true)),
            "{word}"
        );
    }
    for word in ["false", "No", "off", "0"] {
        assert_eq!(
            decode_lax(&boolean, &format!("\"{word}\"")),
            Ok(Value::Bool(false)),
            "{word}"
        );
    }
    // A bool is never a number, and a number is never a bool.
    for input in [r#""maybe""#, r#""""#, "1", "0"] {
        assert!(decode_lax(&boolean, input).is_err(), "{input}");
    }
    assert_eq!(
        decode_lax(&boolean, "true"),
        Ok(Value::Bool(true)),
        "real booleans still work"
    );
}

#[test]
fn reads_an_empty_string_as_none_only_when_the_type_does_not_accept_text() {
    for inner in [integer(), float(), TypeDescription::Bool] {
        let description = plain(optional(inner));
        assert_eq!(decode_lax(&description, r#""""#), Ok(Value::None));
        assert_eq!(decode_lax(&description, "null"), Ok(Value::None));
    }
    let text = plain(optional(string()));
    assert_eq!(
        decode_lax(&text, r#""""#),
        Ok(Value::String(String::new())),
        "str | None keeps the empty string"
    );
    let text_choice = plain(optional(TypeDescription::Union(vec![integer(), string()])));
    assert_eq!(
        decode_lax(&text_choice, r#""""#),
        Ok(Value::String(String::new()))
    );
    let literal = Description {
        root: optional(TypeDescription::Choice(0)),
        ..choice(vec![ChoiceValue::String("a".into())])
    };
    assert_eq!(decode_lax(&literal, r#""""#).unwrap_err().issues.len(), 1);
    let numeric_literal = Description {
        root: optional(TypeDescription::Choice(0)),
        ..choice(vec![ChoiceValue::Integer(1)])
    };
    assert_eq!(decode_lax(&numeric_literal, r#""""#), Ok(Value::None));
}

#[test]
fn reads_choices_from_text() {
    let numbers = choice(vec![ChoiceValue::Integer(1), ChoiceValue::Integer(2)]);
    assert_eq!(decode_lax(&numbers, r#""2""#), Ok(Value::Choice(0, 1)));
    assert_eq!(decode_lax(&numbers, "1.0"), Ok(Value::Choice(0, 0)));
    assert!(decode_lax(&numbers, r#""3""#).is_err());
    // An exact match always wins over a coercion.
    let both = choice(vec![
        ChoiceValue::String("1".into()),
        ChoiceValue::Integer(1),
    ]);
    assert_eq!(decode_lax(&both, r#""1""#), Ok(Value::Choice(0, 0)));
    assert_eq!(decode_lax(&both, "1"), Ok(Value::Choice(0, 1)));
}

#[test]
fn reads_integer_conversions_from_text() {
    let flag = conversion(ConversionInput::Integer);
    assert_eq!(
        decode_lax(&flag, r#""5""#),
        Ok(Value::Converted(0, Box::new(Value::Integer(5))))
    );
    let rejected = decode_lax(&flag, r#""0""#).unwrap_err();
    assert_eq!(rejected.issues[0].code.as_str(), "enum");
    assert!(decode_lax(&flag, r#""x""#).is_err());
    assert!(decode(&flag, r#""5""#).is_err());
}

#[test]
fn picks_a_union_variant_for_text_that_has_no_string_variant() {
    let numeric = plain(TypeDescription::Union(vec![integer(), float()]));
    assert_eq!(decode_lax(&numeric, r#""7""#), Ok(Value::Integer(7)));
    assert_eq!(decode_lax(&numeric, r#""7.5""#), Ok(Value::Float(7.5)));
    let with_bool = plain(TypeDescription::Union(vec![
        integer(),
        TypeDescription::Bool,
    ]));
    assert_eq!(decode_lax(&with_bool, r#""1""#), Ok(Value::Integer(1)));
    assert_eq!(decode_lax(&with_bool, r#""yes""#), Ok(Value::Bool(true)));
    assert!(decode_lax(&with_bool, r#""maybe""#).is_err());
    // A string variant takes the text as it is.
    let with_text = plain(TypeDescription::Union(vec![integer(), string()]));
    assert_eq!(
        decode_lax(&with_text, r#""7""#),
        Ok(Value::String("7".into()))
    );
}

#[test]
fn coerces_inside_containers_and_structures() {
    let ints = plain(TypeDescription::List {
        items: Box::new(integer()),
        constraints: Constraints::default(),
    });
    assert_eq!(
        decode_lax(&ints, r#"["1", 2, "3.0"]"#),
        Ok(Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3)
        ]))
    );
    let tuple = plain(TypeDescription::Collection {
        kind: SequenceKind::Tuple,
        items: Box::new(integer()),
        constraints: Constraints::default(),
    });
    assert_eq!(
        decode_lax(&tuple, r#"["1", "2"]"#),
        Ok(Value::Tuple(vec![Value::Integer(1), Value::Integer(2)]))
    );
    let user = Description {
        structs: vec![structure(
            "User",
            vec![
                field("id", integer()),
                field("admin", TypeDescription::Bool),
                field("score", optional(float())),
            ],
        )],
        ..plain(TypeDescription::Struct(0))
    };
    assert_eq!(
        decode_lax(&user, r#"{"id": "7", "admin": "no", "score": ""}"#),
        Ok(Value::Instance(
            0,
            vec![
                Some(Value::Integer(7)),
                Some(Value::Bool(false)),
                Some(Value::None)
            ]
        ))
    );
    let report = decode_lax(&user, r#"{"id": "x", "admin": "maybe", "score": "1"}"#).unwrap_err();
    assert_eq!(report.kind, ReportKind::Validation);
    let pointers: Vec<_> = report.issues.iter().map(|issue| issue.pointer()).collect();
    assert_eq!(pointers, ["/id", "/admin"]);
}
