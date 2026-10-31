use oxiform_core::description::{Constraints, Description, TagMode, TypeDescription};

use crate::support::{Value, decode, field, integer, issues, string, structure, summary};

fn card_and_pix(root: impl FnOnce(Vec<TypeDescription>) -> TypeDescription) -> Description {
    let mut card = structure("Card", vec![field("number", string())]);
    card.tag = "card".into();
    let mut pix = structure(
        "Pix",
        vec![field("key", string()), field("amount", integer())],
    );
    pix.tag = "pix".into();
    Description {
        root: root(vec![TypeDescription::Struct(0), TypeDescription::Struct(1)]),
        structs: vec![card, pix],
        choices: Vec::new(),
    }
}

fn internal(tag: &str) -> impl FnOnce(Vec<TypeDescription>) -> TypeDescription {
    let mode = TagMode::Internal { tag: tag.into() };
    move |variants| TypeDescription::Tagged { mode, variants }
}

fn pix(key: &str, amount: i64) -> Value {
    Value::Instance(
        1,
        vec![
            Some(Value::String(key.into())),
            Some(Value::Integer(amount)),
        ],
    )
}

#[test]
fn chooses_the_variant_by_the_kind_of_value() {
    let description = crate::support::plain(TypeDescription::Union(vec![
        integer(),
        string(),
        TypeDescription::None,
        TypeDescription::List {
            items: Box::new(integer()),
            constraints: Constraints::default(),
        },
    ]));
    assert_eq!(decode(&description, "1"), Ok(Value::Integer(1)));
    assert_eq!(
        decode(&description, r#""a""#),
        Ok(Value::String("a".into()))
    );
    assert_eq!(decode(&description, "null"), Ok(Value::None));
    assert_eq!(
        decode(&description, "[2]"),
        Ok(Value::List(vec![Value::Integer(2)]))
    );
    assert_eq!(
        summary(&description, "true"),
        [(
            "type",
            String::new(),
            "expected int | str | None | list, received bool".into()
        )]
    );
}

#[test]
fn reads_numbers_without_a_fraction_as_int() {
    let description = crate::support::plain(TypeDescription::Union(vec![
        TypeDescription::Float(Constraints::default()),
        integer(),
    ]));
    assert_eq!(decode(&description, "2"), Ok(Value::Integer(2)));
    assert_eq!(decode(&description, "2.5"), Ok(Value::Float(2.5)));
}

#[test]
fn reads_the_tag_wherever_it_is() {
    let description = card_and_pix(internal("type"));
    let expected = Ok(pix("k", 3));
    assert_eq!(
        decode(&description, r#"{"type": "pix", "key": "k", "amount": 3}"#),
        expected
    );
    assert_eq!(
        decode(&description, r#"{"key": "k", "amount": 3, "type": "pix"}"#),
        expected
    );
    assert_eq!(
        summary(
            &description,
            r#"{"amount": "x", "type": "pix", "key": "k"}"#
        ),
        [(
            "type",
            "/amount".into(),
            "expected int, received str".into()
        )]
    );
}

#[test]
fn reports_a_missing_or_unknown_tag() {
    let description = card_and_pix(internal("type"));
    assert_eq!(
        summary(&description, r#"{"key": "k"}"#),
        [("tag", String::new(), r#"missing the tag key "type""#.into())]
    );
    let unknown = issues(&description, r#"{"type": "cash", "key": "k"}"#);
    assert_eq!(unknown[0].pointer(), "/type");
    assert_eq!(
        unknown[0].message,
        r#"unknown tag "cash", expected one of "card", "pix""#
    );
}

#[test]
fn reads_adjacent_tags_in_any_order() {
    let mode = TagMode::Adjacent {
        tag: "t".into(),
        content: "c".into(),
    };
    let description = card_and_pix(|variants| TypeDescription::Tagged { mode, variants });
    let expected = Ok(pix("k", 1));
    assert_eq!(
        decode(
            &description,
            r#"{"t": "pix", "c": {"key": "k", "amount": 1}}"#
        ),
        expected
    );
    assert_eq!(
        decode(
            &description,
            r#"{"c": {"key": "k", "amount": 1}, "t": "pix"}"#
        ),
        expected
    );
    assert_eq!(
        summary(&description, r#"{"t": "pix"}"#),
        [("missing", "/c".into(), "field required".into())]
    );
}

#[test]
fn reads_external_tags() {
    let description = card_and_pix(|variants| TypeDescription::Tagged {
        mode: TagMode::External,
        variants,
    });
    assert_eq!(
        decode(&description, r#"{"pix": {"key": "k", "amount": 1}}"#),
        Ok(pix("k", 1))
    );
    assert_eq!(
        summary(
            &description,
            r#"{"pix": {"key": "k", "amount": 1}, "card": {}}"#
        )[0]
        .2,
        "expected a map with the tag as its only key"
    );
}

#[test]
fn resolves_structures_that_share_a_tag_field() {
    let mut description = card_and_pix(TypeDescription::Union);
    for structure in &mut description.structs {
        structure.tag_field = Some("kind".into());
    }
    assert_eq!(
        decode(&description, r#"{"kind": "pix", "key": "k", "amount": 2}"#),
        Ok(pix("k", 2))
    );
}

#[test]
fn checks_the_tag_of_a_structure_read_on_its_own() {
    let mut description = card_and_pix(|_| TypeDescription::Struct(1));
    description.structs[1].tag_field = Some("kind".into());
    assert!(decode(&description, r#"{"key": "k", "amount": 2}"#).is_ok());
    assert!(decode(&description, r#"{"kind": "pix", "key": "k", "amount": 2}"#).is_ok());
    assert_eq!(
        summary(&description, r#"{"kind": "card", "key": "k", "amount": 2}"#),
        [("tag", "/kind".into(), r#"expected the tag "pix""#.into())]
    );
}

#[test]
fn tries_untagged_variants_in_order() {
    let description = card_and_pix(TypeDescription::Untagged);
    assert_eq!(
        decode(&description, r#"{"key": "k", "amount": 1}"#),
        Ok(pix("k", 1))
    );
    let failed = issues(&description, r#"{"amount": 1}"#);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].code.as_str(), "union");
    assert_eq!(failed[0].message, "no variant of Card | Pix fits");
}
