use std::convert::Infallible;

use oxiform_core::compiler::compile;
use oxiform_core::description::{
    Constraints, Description, FieldDescription, StructDescription, TypeDescription, UnknownFields,
};
use oxiform_core::executor::{Failure, execute};
use oxiform_core::issues::{Issue, Report};
use oxiform_core::limits::Limits;
use oxiform_core::plan::{ChoiceId, ConversionId, StructId};
use oxiform_core::sink::Sink;
use oxiform_core::temporal::Temporal;
use oxiform_formats::json::JsonReader;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Integer(i64),
    BigInteger(String),
    Float(f64),
    String(String),
    Bytes(Vec<u8>, bool),
    Temporal(Temporal),
    /// A conversion index and the raw value it was built from.
    Converted(usize, Box<Value>),
    List(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    Tuple(Vec<Value>),
    /// The items, and whether the set is frozen.
    Set(Vec<Value>, bool),
    /// A choice index and the index of the value.
    Choice(usize, u32),
    /// A structure index and its fields; `None` for a field that takes its default.
    Instance(usize, Vec<Option<Value>>),
}

pub struct ValueSink;

impl Sink for ValueSink {
    type Value = Value;
    type Error = Infallible;
    type Map = Vec<(Value, Value)>;

    fn none(&mut self) -> Result<Value, Infallible> {
        Ok(Value::None)
    }
    fn bool(&mut self, value: bool) -> Result<Value, Infallible> {
        Ok(Value::Bool(value))
    }
    fn integer(&mut self, value: i64) -> Result<Value, Infallible> {
        Ok(Value::Integer(value))
    }
    fn big_integer(&mut self, digits: &str) -> Result<Value, Infallible> {
        Ok(Value::BigInteger(digits.into()))
    }
    fn float(&mut self, value: f64) -> Result<Value, Infallible> {
        Ok(Value::Float(value))
    }
    fn string(&mut self, value: &str) -> Result<Value, Infallible> {
        Ok(Value::String(value.into()))
    }
    fn bytes(&mut self, value: &[u8], mutable: bool) -> Result<Value, Infallible> {
        Ok(Value::Bytes(value.to_vec(), mutable))
    }
    fn temporal(&mut self, value: Temporal) -> Result<Value, Infallible> {
        Ok(Value::Temporal(value))
    }
    /// Rejects the raw string "bad" and the integer 0, so tests can reach the issue.
    fn convert(
        &mut self,
        conversion: ConversionId,
        raw: Value,
    ) -> Result<Option<Value>, Infallible> {
        if matches!(&raw, Value::String(text) if text == "bad") || raw == Value::Integer(0) {
            return Ok(None);
        }
        Ok(Some(Value::Converted(conversion.index(), Box::new(raw))))
    }
    fn list(&mut self, items: Vec<Value>) -> Result<Value, Infallible> {
        Ok(Value::List(items))
    }
    fn tuple(&mut self, items: Vec<Value>) -> Result<Value, Infallible> {
        Ok(Value::Tuple(items))
    }
    fn set(&mut self, items: Vec<Value>, frozen: bool) -> Result<Value, Infallible> {
        Ok(Value::Set(items, frozen))
    }
    fn choice(&mut self, choice: ChoiceId, index: u32) -> Result<Value, Infallible> {
        Ok(Value::Choice(choice.index(), index))
    }
    fn map_new(&mut self) -> Result<Self::Map, Infallible> {
        Ok(Vec::new())
    }
    fn map_insert(
        &mut self,
        map: &mut Self::Map,
        key: &str,
        value: Value,
    ) -> Result<bool, Infallible> {
        let key = Value::String(key.into());
        if map.iter().any(|(existing, _)| *existing == key) {
            return Ok(false);
        }
        map.push((key, value));
        Ok(true)
    }
    fn map_finish(&mut self, map: Self::Map) -> Result<Value, Infallible> {
        Ok(Value::Dict(map))
    }
    fn instance(
        &mut self,
        structure: StructId,
        fields: &mut [Option<Value>],
    ) -> Result<Value, Infallible> {
        Ok(Value::Instance(
            structure.index(),
            fields.iter_mut().map(Option::take).collect(),
        ))
    }
}

pub fn plain(root: TypeDescription) -> Description {
    Description {
        root,
        structs: Vec::new(),
        choices: Vec::new(),
        conversions: Vec::new(),
    }
}

pub fn field(name: &str, value_type: TypeDescription) -> FieldDescription {
    FieldDescription {
        name: name.into(),
        wire_name: name.into(),
        aliases: Vec::new(),
        value_type,
        has_default: false,
    }
}

pub fn structure(name: &str, fields: Vec<FieldDescription>) -> StructDescription {
    StructDescription {
        name: name.into(),
        fields,
        unknown_fields: UnknownFields::Ignore,
        tag: name.into(),
        tag_field: None,
    }
}

pub fn integer() -> TypeDescription {
    TypeDescription::Integer(Constraints::default())
}

pub fn string() -> TypeDescription {
    TypeDescription::String(Constraints::default())
}

pub fn decode_with(
    description: &Description,
    input: &str,
    limits: &Limits,
) -> Result<Value, Report> {
    let plan = compile(description).expect("the description compiles");
    let mut reader = JsonReader::new(input, limits.max_depth);
    match execute(&plan, &mut reader, &mut ValueSink, limits) {
        Ok(value) => Ok(value),
        Err(Failure::Invalid(report)) => Err(report),
        Err(Failure::Sink(never)) => match never {},
    }
}

pub fn decode(description: &Description, input: &str) -> Result<Value, Report> {
    decode_with(description, input, &Limits::default())
}

pub fn issues(description: &Description, input: &str) -> Vec<Issue> {
    decode(description, input)
        .expect_err("the input should be invalid")
        .issues
}

/// `(code, pointer, message)` of each issue.
pub fn summary(description: &Description, input: &str) -> Vec<(&'static str, String, String)> {
    issues(description, input)
        .into_iter()
        .map(|issue| (issue.code.as_str(), issue.pointer(), issue.message))
        .collect()
}
