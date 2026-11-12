//! Lax mode: text is read as the type the field asks for (section 4.5 of the spec).

use oxiform_core::description::{
    Bound, ChoiceDescription, ChoiceValue, Constraints, ConversionDescription, ConversionInput,
    Description, SequenceKind, TemporalKind, TypeDescription,
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
    assert_eq!(
        decode_lax(&literal, r#""""#),
        Ok(Value::None),
        "a Literal without \"\" has no empty form"
    );
    assert_eq!(decode_lax(&literal, r#""a""#), Ok(Value::Choice(0, 0)));
    let empty_literal = Description {
        root: optional(TypeDescription::Choice(0)),
        ..choice(vec![ChoiceValue::String(String::new())])
    };
    assert_eq!(decode_lax(&empty_literal, r#""""#), Ok(Value::Choice(0, 0)));
    for temporal in [TemporalKind::Date, TemporalKind::DateTime] {
        let description = plain(optional(TypeDescription::Temporal(temporal)));
        assert_eq!(decode_lax(&description, r#""""#), Ok(Value::None));
    }
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

fn union_of(variants: Vec<TypeDescription>) -> Description {
    plain(TypeDescription::Union(variants))
}

#[test]
fn reads_an_empty_string_as_none_in_unions_with_none() {
    let none = || TypeDescription::None;
    for variants in [
        vec![integer(), float(), none()],
        vec![integer(), TypeDescription::Bool, none()],
        vec![float(), none()],
        vec![TypeDescription::Bool, none()],
    ] {
        let description = union_of(variants);
        assert_eq!(decode_lax(&description, r#""""#), Ok(Value::None));
        assert_eq!(decode_lax(&description, "null"), Ok(Value::None));
        assert!(
            decode(&description, r#""""#).is_err(),
            "strict mode keeps it an error"
        );
    }
    // The nested form of the same type reads the same text the same way.
    let nested = plain(optional(TypeDescription::Union(vec![integer(), float()])));
    assert_eq!(decode_lax(&nested, r#""""#), Ok(Value::None));
    assert_eq!(decode_lax(&nested, r#""2.5""#), Ok(Value::Float(2.5)));
    // A string variant still keeps the empty string, and so does a Literal that has it.
    let with_text = union_of(vec![string(), none()]);
    assert_eq!(
        decode_lax(&with_text, r#""""#),
        Ok(Value::String(String::new()))
    );
    let literal = Description {
        root: TypeDescription::Union(vec![integer(), TypeDescription::Choice(0), none()]),
        ..choice(vec![ChoiceValue::String("auto".into())])
    };
    assert_eq!(decode_lax(&literal, r#""""#), Ok(Value::None));
    assert_eq!(decode_lax(&literal, r#""auto""#), Ok(Value::Choice(0, 0)));
    assert_eq!(decode_lax(&literal, r#""4""#), Ok(Value::Integer(4)));
    assert!(decode_lax(&literal, r#""x""#).is_err());
}

#[test]
fn a_literal_member_wins_over_a_number_in_a_union_and_numbers_still_coerce() {
    let auto = Description {
        root: TypeDescription::Union(vec![integer(), TypeDescription::Choice(0)]),
        ..choice(vec![ChoiceValue::String("auto".into())])
    };
    let reversed = Description {
        root: TypeDescription::Union(vec![TypeDescription::Choice(0), integer()]),
        ..choice(vec![ChoiceValue::String("auto".into())])
    };
    for description in [&auto, &reversed] {
        assert_eq!(decode_lax(description, r#""4""#), Ok(Value::Integer(4)));
        assert_eq!(decode_lax(description, r#""-4.0""#), Ok(Value::Integer(-4)));
        assert_eq!(decode_lax(description, "4"), Ok(Value::Integer(4)));
        assert_eq!(
            decode_lax(description, r#""auto""#),
            Ok(Value::Choice(0, 0))
        );
        assert!(
            decode(description, r#""4""#).is_err(),
            "strict stays strict"
        );
        // Text that is neither reports the error of the text variant.
        let rejected = decode_lax(description, r#""4.5""#).unwrap_err();
        assert_eq!(rejected.issues.len(), 1);
        assert_eq!(rejected.issues[0].code.as_str(), "literal");
    }
    // An exact member beats a coercion when both could read the text.
    let both = Description {
        root: TypeDescription::Union(vec![TypeDescription::Bool, TypeDescription::Choice(0)]),
        ..choice(vec![ChoiceValue::String("yes".into())])
    };
    assert_eq!(decode_lax(&both, r#""yes""#), Ok(Value::Choice(0, 0)));
    assert_eq!(decode_lax(&both, r#""on""#), Ok(Value::Bool(true)));
    assert_eq!(decode_lax(&both, r#""0""#), Ok(Value::Bool(false)));
    // Constraints still apply to the coerced value.
    let bounded = Description {
        root: TypeDescription::Union(vec![
            TypeDescription::Integer(Constraints {
                minimum: Some(Bound::Integer(10)),
                ..Constraints::default()
            }),
            TypeDescription::Choice(0),
        ]),
        ..choice(vec![ChoiceValue::String("auto".into())])
    };
    assert_eq!(
        summary_codes(&bounded, r#""4""#),
        ["too_small"],
        "the text is read as the integer and then checked"
    );
}

fn summary_codes(description: &Description, input: &str) -> Vec<&'static str> {
    decode_lax(description, input)
        .unwrap_err()
        .issues
        .iter()
        .map(|issue| issue.code.as_str())
        .collect()
}

#[test]
fn text_that_has_the_form_of_a_date_is_a_date_and_other_text_is_a_number() {
    let date = TypeDescription::Temporal(TemporalKind::Date);
    for variants in [
        vec![integer(), date.clone()],
        vec![date.clone(), integer()],
        vec![date.clone(), TypeDescription::Bool],
    ] {
        let description = union_of(variants);
        let read = decode_lax(&description, r#""2026-01-31""#).unwrap();
        assert!(matches!(read, Value::Temporal(_)), "{read:?}");
    }
    let with_integer = union_of(vec![integer(), date.clone()]);
    assert_eq!(decode_lax(&with_integer, r#""4""#), Ok(Value::Integer(4)));
    assert_eq!(decode_lax(&with_integer, "4"), Ok(Value::Integer(4)));
    let rejected = decode_lax(&with_integer, r#""soon""#).unwrap_err();
    assert_eq!(rejected.issues[0].code.as_str(), "format");
    assert!(decode(&with_integer, r#""4""#).is_err());
}

#[test]
fn a_str_variant_still_takes_every_string_next_to_a_literal() {
    let description = Description {
        root: TypeDescription::Union(vec![integer(), string()]),
        ..plain(TypeDescription::Any)
    };
    assert_eq!(
        decode_lax(&description, r#""4""#),
        Ok(Value::String("4".into()))
    );
}
