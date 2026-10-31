use oxiform_core::description::{Constraints, Description, TypeDescription, UnknownFields};

use crate::support::{Value, decode, field, integer, string, structure, summary};

fn user() -> Description {
    let mut age = field("age", TypeDescription::Optional(Box::new(integer())));
    age.has_default = true;
    let mut name = field("name", string());
    name.wire_name = "fullName".into();
    name.aliases = vec!["name".into()];
    Description {
        root: TypeDescription::Struct(0),
        structs: vec![structure(
            "User",
            vec![
                field("id", integer()),
                name,
                age,
                field(
                    "friends",
                    TypeDescription::List {
                        items: Box::new(TypeDescription::Struct(0)),
                        constraints: Constraints::default(),
                    },
                ),
            ],
        )],
    }
}

#[test]
fn reads_fields_by_wire_name_in_any_order() {
    assert_eq!(
        decode(&user(), r#"{"friends": [], "fullName": "Rex", "id": 1}"#),
        Ok(Value::Instance(
            0,
            vec![
                Some(Value::Integer(1)),
                Some(Value::String("Rex".into())),
                None,
                Some(Value::List(Vec::new()))
            ]
        ))
    );
}

#[test]
fn reads_self_referencing_structures() {
    let decoded = decode(
        &user(),
        r#"{"id": 1, "fullName": "a", "friends": [{"id": 2, "fullName": "b", "friends": []}]}"#,
    );
    let Ok(Value::Instance(0, fields)) = decoded else {
        panic!("expected a user, got {decoded:?}")
    };
    assert!(matches!(&fields[3], Some(Value::List(friends)) if friends.len() == 1));
}

#[test]
fn reports_missing_and_invalid_fields_together() {
    assert_eq!(
        summary(&user(), r#"{"id": "1", "friends": [{"id": 2}]}"#),
        [
            ("type", "/id".into(), "expected int, received str".into()),
            (
                "missing",
                "/friends/0/fullName".into(),
                "field required".into()
            ),
            (
                "missing",
                "/friends/0/friends".into(),
                "field required".into()
            ),
            ("missing", "/fullName".into(), "field required".into()),
        ]
    );
}

#[test]
fn ignores_or_forbids_unknown_fields() {
    let input = r#"{"id": 1, "fullName": "a", "friends": [], "extra": {"deep": [1]}}"#;
    assert!(decode(&user(), input).is_ok());
    let mut strict = user();
    strict.structs[0].unknown_fields = UnknownFields::Forbid;
    assert_eq!(
        summary(&strict, input),
        [("unknown_field", "/extra".into(), "unknown field".into())]
    );
}

#[test]
fn rejects_duplicate_fields() {
    assert_eq!(
        summary(
            &user(),
            r#"{"id": 1, "id": 2, "fullName": "a", "friends": []}"#
        ),
        [("duplicate_key", "/id".into(), "duplicate key".into())]
    );
}

#[test]
fn reads_a_field_from_an_alias() {
    let decoded = decode(&user(), r#"{"id": 1, "name": "Rex", "friends": []}"#);
    let Ok(Value::Instance(0, fields)) = decoded else {
        panic!("expected a user, got {decoded:?}")
    };
    assert_eq!(fields[1], Some(Value::String("Rex".into())));
    assert_eq!(
        summary(
            &user(),
            r#"{"id": 1, "fullName": "a", "name": "b", "friends": []}"#
        ),
        [("duplicate_key", "/name".into(), "duplicate key".into())]
    );
}

#[test]
fn names_the_structure_on_a_type_mismatch() {
    assert_eq!(summary(&user(), "[]")[0].2, "expected User, received list");
}
