use oxiform_core::description::{
    ChoiceDescription, ChoiceValue, Constraints, Description, SequenceKind, TypeDescription,
};

use crate::support::{Value, decode, integer, plain, string, summary};

fn literal(values: Vec<ChoiceValue>, is_enum: bool) -> Description {
    Description {
        root: TypeDescription::Choice(0),
        structs: Vec::new(),
        choices: vec![ChoiceDescription {
            name: "Choice".into(),
            values,
            is_enum,
        }],
    }
}

#[test]
fn reads_each_kind_of_choice() {
    let description = literal(
        vec![
            ChoiceValue::String("a".into()),
            ChoiceValue::Integer(7),
            ChoiceValue::Bool(false),
            ChoiceValue::Null,
        ],
        false,
    );
    assert_eq!(decode(&description, r#""a""#), Ok(Value::Choice(0, 0)));
    assert_eq!(decode(&description, "7"), Ok(Value::Choice(0, 1)));
    assert_eq!(decode(&description, "false"), Ok(Value::Choice(0, 2)));
    assert_eq!(decode(&description, "null"), Ok(Value::Choice(0, 3)));
    assert_eq!(
        summary(&description, "7.0"),
        [(
            "literal",
            String::new(),
            "expected one of 'a', 7, False, None".into()
        )]
    );
}

#[test]
fn reports_enum_failures_as_enum() {
    let description = literal(vec![ChoiceValue::String("red".into())], true);
    assert_eq!(summary(&description, r#""blue""#)[0].0, "enum");
    assert_eq!(summary(&description, "[1]")[0].0, "enum");
}

#[test]
fn reads_fixed_tuples_position_by_position() {
    let description = plain(TypeDescription::Tuple(vec![integer(), string()]));
    assert_eq!(
        decode(&description, r#"[1, "a"]"#),
        Ok(Value::Tuple(vec![
            Value::Integer(1),
            Value::String("a".into())
        ]))
    );
    assert_eq!(
        summary(&description, r#"["a", "b", 3]"#),
        [
            ("type", "/0".into(), "expected int, received str".into()),
            (
                "too_long",
                String::new(),
                "length 3 is more than the maximum 2".into()
            ),
        ]
    );
}

#[test]
fn reads_sets_and_tuples_of_any_length() {
    let collection = |kind| {
        plain(TypeDescription::Collection {
            kind,
            items: Box::new(integer()),
            constraints: Constraints::default(),
        })
    };
    assert_eq!(
        decode(&collection(SequenceKind::FrozenSet), "[1]"),
        Ok(Value::Set(vec![Value::Integer(1)], true))
    );
    assert_eq!(
        decode(&collection(SequenceKind::Tuple), "[]"),
        Ok(Value::Tuple(Vec::new()))
    );
    assert_eq!(
        summary(&collection(SequenceKind::Set), "{}")[0].2,
        "expected set, received dict"
    );
}
