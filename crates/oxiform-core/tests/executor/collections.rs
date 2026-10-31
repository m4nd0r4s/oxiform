use oxiform_core::description::{Constraints, TypeDescription};

use crate::support::{Value, decode, integer, plain, summary};

fn list_of(items: TypeDescription) -> TypeDescription {
    TypeDescription::List {
        items: Box::new(items),
        constraints: Constraints::default(),
    }
}

#[test]
fn reads_lists_and_dicts() {
    assert_eq!(
        decode(&plain(list_of(integer())), "[1, 2]"),
        Ok(Value::List(vec![Value::Integer(1), Value::Integer(2)]))
    );
    let dict = plain(TypeDescription::Dict {
        values: Box::new(integer()),
        constraints: Constraints::default(),
    });
    assert_eq!(
        decode(&dict, r#"{"a": 1}"#),
        Ok(Value::Dict(vec![(
            Value::String("a".into()),
            Value::Integer(1)
        )]))
    );
}

#[test]
fn reports_every_bad_item_with_its_index() {
    assert_eq!(
        summary(&plain(list_of(integer())), r#"[1, "x", 3, null]"#),
        [
            ("type", "/1".into(), "expected int, received str".into()),
            ("type", "/3".into(), "expected int, received None".into()),
        ]
    );
}

#[test]
fn skips_nested_values_of_the_wrong_type() {
    assert_eq!(
        summary(
            &plain(list_of(integer())),
            r#"[[1, [2]], {"a": [3]}, 4, "y"]"#
        ),
        [
            ("type", "/0".into(), "expected int, received list".into()),
            ("type", "/1".into(), "expected int, received dict".into()),
            ("type", "/3".into(), "expected int, received str".into()),
        ]
    );
}

#[test]
fn checks_collection_lengths() {
    let short = plain(TypeDescription::List {
        items: Box::new(integer()),
        constraints: Constraints {
            max_length: Some(2),
            ..Constraints::default()
        },
    });
    assert_eq!(
        summary(&short, "[1, 2, 3]"),
        [(
            "too_long",
            String::new(),
            "length 3 is more than the maximum 2".into()
        )]
    );
}

#[test]
fn rejects_duplicate_keys() {
    let dict = plain(TypeDescription::Dict {
        values: Box::new(integer()),
        constraints: Constraints::default(),
    });
    assert_eq!(
        summary(&dict, r#"{"a": 1, "a": 2}"#),
        [("duplicate_key", "/a".into(), "duplicate key".into())]
    );
    assert_eq!(
        summary(&plain(TypeDescription::Any), r#"{"a": 1, "a": [2]}"#)[0].0,
        "duplicate_key"
    );
}

#[test]
fn escapes_keys_in_json_pointers() {
    let dict = plain(TypeDescription::Dict {
        values: Box::new(integer()),
        constraints: Constraints::default(),
    });
    assert_eq!(summary(&dict, r#"{"a/b~c": "x"}"#)[0].1, "/a~1b~0c");
}
