use super::compile;
use crate::description::{
    Bound, Constraints, Description, FieldDescription, Literal, StructDescription, TypeDescription,
    UnknownFields,
};
use crate::plan::Node;

fn field(name: &str, value_type: TypeDescription) -> FieldDescription {
    FieldDescription {
        name: name.into(),
        wire_name: name.into(),
        aliases: Vec::new(),
        value_type,
        has_default: false,
    }
}

fn single_struct(fields: Vec<FieldDescription>) -> Description {
    Description {
        root: TypeDescription::Struct(0),
        structs: vec![StructDescription {
            name: "User".into(),
            fields,
            unknown_fields: UnknownFields::Ignore,
        }],
    }
}

fn error_message(description: &Description) -> String {
    compile(description)
        .expect_err("the declaration should be rejected")
        .to_string()
}

#[test]
fn compiles_a_structure_with_constrained_fields() {
    let description = single_struct(vec![
        field(
            "age",
            TypeDescription::Integer(Constraints {
                minimum: Some(Bound::Integer(0)),
                maximum: Some(Bound::Integer(150)),
                ..Constraints::default()
            }),
        ),
        field(
            "name",
            TypeDescription::String(Constraints {
                min_length: Some(1),
                pattern: Some("^[a-z]+$".into()),
                ..Constraints::default()
            }),
        ),
        field(
            "tags",
            TypeDescription::List {
                items: Box::new(TypeDescription::String(Constraints::default())),
                constraints: Constraints::default(),
            },
        ),
    ]);
    let plan = compile(&description).unwrap();
    let Node::Struct(struct_id) = plan.node(plan.root()) else {
        panic!("the root should be a structure")
    };
    let user = plan.struct_plan(*struct_id);
    assert_eq!(user.field_index("name"), Some(1));
    assert_eq!(user.field_index("missing"), None);
    assert!(
        matches!(plan.node(user.fields[0].value), Node::Integer(checks) if checks.minimum == Some(Bound::Integer(0)))
    );
    let Node::String(checks) = plan.node(user.fields[1].value) else {
        panic!("name should be a string")
    };
    assert!(checks.pattern.as_ref().unwrap().is_match("rex"));
}

#[test]
fn compiles_self_referencing_structures() {
    let description = single_struct(vec![field(
        "children",
        TypeDescription::List {
            items: Box::new(TypeDescription::Struct(0)),
            constraints: Constraints::default(),
        },
    )]);
    assert!(compile(&description).is_ok());
}

#[test]
fn rejects_constraints_on_the_wrong_type() {
    let length = Constraints {
        min_length: Some(1),
        ..Constraints::default()
    };
    let minimum = Constraints {
        minimum: Some(Bound::Integer(0)),
        ..Constraints::default()
    };
    let pattern = Constraints {
        pattern: Some("x".into()),
        ..Constraints::default()
    };
    assert_eq!(
        error_message(&single_struct(vec![field(
            "age",
            TypeDescription::Integer(length)
        )])),
        "User.age: Len does not apply to int"
    );
    assert_eq!(
        error_message(&single_struct(vec![field(
            "name",
            TypeDescription::String(minimum.clone())
        )])),
        "User.name: Ge does not apply to str"
    );
    assert_eq!(
        error_message(&single_struct(vec![field(
            "tags",
            TypeDescription::List {
                items: Box::new(TypeDescription::Any),
                constraints: pattern
            }
        )])),
        "User.tags: Pattern does not apply to list"
    );
}

#[test]
fn rejects_contradictory_limits() {
    let bounds = Constraints {
        minimum: Some(Bound::Float(10.5)),
        maximum: Some(Bound::Integer(10)),
        ..Constraints::default()
    };
    assert_eq!(
        error_message(&single_struct(vec![field(
            "score",
            TypeDescription::Float(bounds)
        )])),
        "User.score: the Ge limit is greater than the Le limit"
    );
    let lengths = Constraints {
        min_length: Some(5),
        max_length: Some(2),
        ..Constraints::default()
    };
    assert_eq!(
        error_message(&single_struct(vec![field(
            "name",
            TypeDescription::String(lengths)
        )])),
        "User.name: the Len minimum is greater than its maximum"
    );
}

#[test]
fn rejects_regex_features_that_need_backtracking() {
    let lookahead = Constraints {
        pattern: Some("^(?=a)".into()),
        ..Constraints::default()
    };
    let message = error_message(&single_struct(vec![field(
        "name",
        TypeDescription::String(lookahead),
    )]));
    assert!(
        message.starts_with("User.name: invalid Pattern \"^(?=a)\""),
        "{message}"
    );
}

#[test]
fn checks_one_of_values_against_the_type() {
    let mixed = Constraints {
        one_of: Some(vec![Literal::String("a".into()), Literal::Integer(1)]),
        ..Constraints::default()
    };
    assert_eq!(
        error_message(&single_struct(vec![field(
            "kind",
            TypeDescription::String(mixed)
        )])),
        "User.kind: OneOf value 1 is not a str"
    );
    let empty = Constraints {
        one_of: Some(Vec::new()),
        ..Constraints::default()
    };
    assert_eq!(
        error_message(&single_struct(vec![field(
            "level",
            TypeDescription::Integer(empty)
        )])),
        "User.level: OneOf needs at least one value"
    );
}

#[test]
fn rejects_two_fields_read_from_the_same_key() {
    let mut second = field(
        "identifier",
        TypeDescription::Integer(Constraints::default()),
    );
    second.wire_name = "id".into();
    let description = single_struct(vec![
        field("id", TypeDescription::Integer(Constraints::default())),
        second,
    ]);
    assert_eq!(
        error_message(&description),
        "User.identifier: another field is already read from the key \"id\""
    );
}

#[test]
fn rejects_an_alias_that_another_field_is_read_from() {
    let mut second = field(
        "identifier",
        TypeDescription::Integer(Constraints::default()),
    );
    second.aliases = vec!["key".into(), "id".into()];
    let description = single_struct(vec![
        field("id", TypeDescription::Integer(Constraints::default())),
        second,
    ]);
    assert_eq!(
        error_message(&description),
        "User.identifier: another field is already read from the key \"id\""
    );
}

#[test]
fn rejects_unknown_structure_indices() {
    let description = Description {
        root: TypeDescription::Struct(3),
        structs: Vec::new(),
    };
    assert_eq!(
        error_message(&description),
        "root: unknown structure index 3"
    );
}
