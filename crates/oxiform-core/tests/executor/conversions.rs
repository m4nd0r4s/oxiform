use oxiform_core::description::{
    Constraints, ConversionDescription, ConversionInput, Description, TypeDescription,
};

use crate::support::{Value, decode, plain, string, summary};

fn converted(input: ConversionInput) -> Description {
    Description {
        conversions: vec![ConversionDescription {
            name: "Thing".into(),
            format: "thing".into(),
            input,
        }],
        ..plain(TypeDescription::Converted(0))
    }
}

fn bytes(constraints: Constraints) -> Description {
    plain(TypeDescription::Bytes {
        mutable: false,
        constraints,
    })
}

fn raw(value: Value) -> Value {
    Value::Converted(0, Box::new(value))
}

#[test]
fn reads_bytes_from_base64() {
    let description = bytes(Constraints::default());
    assert_eq!(
        decode(&description, r#""aGk=""#),
        Ok(Value::Bytes(b"hi".to_vec(), false))
    );
    assert_eq!(
        summary(&description, r#""aGk""#),
        [("format", String::new(), "invalid base64".into())]
    );
    assert_eq!(
        summary(&description, "[]"),
        [(
            "type",
            String::new(),
            "expected bytes, received list".into()
        )]
    );
}

#[test]
fn checks_the_length_of_the_decoded_bytes() {
    let description = bytes(Constraints {
        max_length: Some(1),
        ..Constraints::default()
    });
    assert_eq!(
        summary(&description, r#""aGk=""#),
        [(
            "too_long",
            String::new(),
            "length 2 is more than the maximum 1".into()
        )]
    );
}

#[test]
fn converts_strings_and_reports_rejected_ones() {
    let description = converted(ConversionInput::String);
    assert_eq!(
        decode(&description, r#""x""#),
        Ok(raw(Value::String("x".into())))
    );
    assert_eq!(
        summary(&description, r#""bad""#),
        [("format", String::new(), "invalid Thing".into())]
    );
    assert_eq!(
        summary(&description, "1"),
        [("type", String::new(), "expected Thing, received int".into())]
    );
}

#[test]
fn converts_integers_and_reports_rejected_ones_as_enum() {
    let description = converted(ConversionInput::Integer);
    assert_eq!(decode(&description, "5"), Ok(raw(Value::Integer(5))));
    assert_eq!(
        summary(&description, "0"),
        [("enum", String::new(), "0 is not a valid Thing".into())]
    );
    assert_eq!(
        summary(&description, "1.5"),
        [(
            "type",
            String::new(),
            "expected Thing, received float".into()
        )]
    );
}

#[test]
fn reads_decimals_from_numbers_and_strings_as_text() {
    let description = converted(ConversionInput::Decimal);
    for (input, text) in [("1.10", "1.10"), (r#""-2e3""#, "-2e3"), (r#""NaN""#, "NaN")] {
        assert_eq!(
            decode(&description, input),
            Ok(raw(Value::String(text.into())))
        );
    }
    assert_eq!(
        summary(&description, r#""1,5""#),
        [("format", String::new(), "invalid Thing".into())]
    );
    assert_eq!(
        summary(&description, "true"),
        [(
            "type",
            String::new(),
            "expected Thing, received bool".into()
        )]
    );
}

#[test]
fn tells_conversions_apart_from_other_kinds_in_a_union() {
    let description = Description {
        conversions: vec![ConversionDescription {
            name: "Decimal".into(),
            format: "decimal".into(),
            input: ConversionInput::Decimal,
        }],
        ..plain(TypeDescription::Union(vec![
            TypeDescription::Converted(0),
            TypeDescription::None,
        ]))
    };
    assert_eq!(
        decode(&description, "2"),
        Ok(raw(Value::String("2".into())))
    );
    assert_eq!(decode(&description, "null"), Ok(Value::None));
    let clash = Description {
        root: TypeDescription::Union(vec![TypeDescription::Converted(0), string()]),
        ..description
    };
    let error = oxiform_core::compiler::compile(&clash).expect_err("the union is ambiguous");
    assert!(
        error
            .message
            .starts_with("Decimal and str are both read from")
    );
}
