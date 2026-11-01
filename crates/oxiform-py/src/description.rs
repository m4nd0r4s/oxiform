//! Reads the description built by `oxiform._introspection` into the Rust description, keeping
//! what the sink needs to create instances.

use oxiform_core::description::{
    Bound as NumericBound, ChoiceDescription, ChoiceValue, Constraints, ConversionDescription,
    ConversionInput, Description, FieldDescription, Literal, SequenceKind, StructDescription,
    TagMode, TypeDescription, UnknownFields,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyInt, PyString, PyTuple, PyType};

use crate::slots::slot_offset;

/// What a missing field takes.
pub(crate) enum FieldDefault {
    Required,
    Value(Py<PyAny>),
    Factory(Py<PyAny>),
}

/// How a field is stored on an instance.
pub(crate) enum FieldSetter {
    /// A `__slots__` member: the value is written at this byte offset in the object.
    Slot(isize),
    /// Anything else: set through the generic attribute setter.
    Attribute(Py<PyString>),
}

/// How to create an instance of one class from the fields read.
pub(crate) struct Layout {
    pub(crate) cls: Py<PyType>,
    pub(crate) members: Vec<Member>,
    pub(crate) has_post_init: bool,
}

/// One attribute of an instance and where its value comes from.
pub(crate) struct Member {
    pub(crate) setter: FieldSetter,
    pub(crate) source: MemberSource,
}

pub(crate) enum MemberSource {
    /// The field at this index of the structure, or its default when the key was missing.
    Field(usize),
    /// A field that is not read: always its default.
    Default(FieldDefault),
    /// A flattened model, built from fields of the same structure.
    Flatten(Layout),
}

/// How to create instances of one structure.
pub(crate) struct StructRuntime {
    pub(crate) layout: Layout,
    /// The default of each field read, in field order.
    pub(crate) defaults: Vec<FieldDefault>,
}

/// What the sink needs besides the plan: how to create instances of each structure, the
/// Python object of each value of each set of choices, and the callable of each conversion.
pub(crate) struct Runtime {
    pub(crate) structs: Vec<StructRuntime>,
    pub(crate) choices: Vec<Vec<Py<PyAny>>>,
    pub(crate) conversions: Vec<Py<PyAny>>,
}

/// Converts a Python `Description`.
pub(crate) fn read_description(description: &Bound<'_, PyAny>) -> PyResult<(Description, Runtime)> {
    let root = read_type(&description.getattr("root")?)?;
    let mut structs = Vec::new();
    let mut runtime = Runtime {
        structs: Vec::new(),
        choices: Vec::new(),
        conversions: Vec::new(),
    };
    for struct_description in description.getattr("structs")?.try_iter()? {
        let (struct_description, struct_runtime) = read_struct(&struct_description?)?;
        structs.push(struct_description);
        runtime.structs.push(struct_runtime);
    }
    let mut choices = Vec::new();
    for choice in description.getattr("choices")?.try_iter()? {
        let choice = choice?;
        let values = choice
            .getattr("values")?
            .try_iter()?
            .map(|value| read_choice_value(&value?))
            .collect::<PyResult<Vec<_>>>()?;
        choices.push(ChoiceDescription {
            name: choice.getattr("name")?.extract()?,
            values,
            is_enum: choice.getattr("is_enum")?.extract()?,
        });
        runtime.choices.push(choice.getattr("objects")?.extract()?);
    }
    let mut conversions = Vec::new();
    for conversion in description.getattr("conversions")?.try_iter()? {
        let conversion = conversion?;
        let input = match conversion.getattr("input")?.extract::<String>()?.as_str() {
            "int" => ConversionInput::Integer,
            "decimal" => ConversionInput::Decimal,
            _ => ConversionInput::String,
        };
        conversions.push(ConversionDescription {
            name: conversion.getattr("name")?.extract()?,
            format: conversion.getattr("format")?.extract()?,
            input,
        });
        runtime
            .conversions
            .push(conversion.getattr("convert")?.unbind());
    }
    Ok((
        Description {
            root,
            structs,
            choices,
            conversions,
        },
        runtime,
    ))
}

fn read_choice_value(value: &Bound<'_, PyAny>) -> PyResult<ChoiceValue> {
    Ok(if value.is_none() {
        ChoiceValue::Null
    } else if value.is_instance_of::<PyBool>() {
        ChoiceValue::Bool(value.extract()?)
    } else if value.is_instance_of::<PyInt>() {
        ChoiceValue::Integer(value.extract()?)
    } else {
        ChoiceValue::String(value.extract()?)
    })
}

fn read_struct(description: &Bound<'_, PyAny>) -> PyResult<(StructDescription, StructRuntime)> {
    let mut fields = Vec::new();
    let mut defaults = Vec::new();
    for field in description.getattr("fields")?.try_iter()? {
        let field = field?;
        let default = read_default(&field.getattr("default")?)?;
        fields.push(FieldDescription {
            name: field.getattr("name")?.extract()?,
            wire_name: field.getattr("wire_name")?.extract()?,
            aliases: field.getattr("aliases")?.extract()?,
            value_type: read_type(&field.getattr("value_type")?)?,
            has_default: !matches!(default, FieldDefault::Required),
        });
        defaults.push(default);
    }
    let unknown_fields = match description
        .getattr("unknown_fields")?
        .extract::<String>()?
        .as_str()
    {
        "forbid" => UnknownFields::Forbid,
        _ => UnknownFields::Ignore,
    };
    let struct_description = StructDescription {
        name: description.getattr("name")?.extract()?,
        fields,
        unknown_fields,
        tag: description.getattr("tag")?.extract()?,
        tag_field: description.getattr("tag_field")?.extract()?,
    };
    let runtime = StructRuntime {
        layout: read_layout(&description.getattr("layout")?)?,
        defaults,
    };
    Ok((struct_description, runtime))
}

fn read_layout(layout: &Bound<'_, PyAny>) -> PyResult<Layout> {
    let cls = layout.getattr("cls")?.cast_into::<PyType>()?;
    let mut members = Vec::new();
    for member in layout.getattr("members")?.try_iter()? {
        let member = member?.cast_into::<PyTuple>()?;
        let kind: String = member.get_item(0)?.extract()?;
        let name: String = member.get_item(1)?.extract()?;
        let detail = member.get_item(2)?;
        let source = match kind.as_str() {
            "field" => MemberSource::Field(detail.extract()?),
            "default" => MemberSource::Default(read_default(&detail)?),
            "flatten" => MemberSource::Flatten(read_layout(&detail)?),
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown member kind {other:?}"
                )));
            }
        };
        members.push(Member {
            setter: field_setter(&cls, &name)?,
            source,
        });
    }
    Ok(Layout {
        has_post_init: layout.getattr("has_post_init")?.extract()?,
        cls: cls.unbind(),
        members,
    })
}

/// How to store field `name` on instances of `cls`.
fn field_setter(cls: &Bound<'_, PyType>, name: &str) -> PyResult<FieldSetter> {
    Ok(match slot_offset(cls, name)? {
        Some(offset) => FieldSetter::Slot(offset),
        None => FieldSetter::Attribute(PyString::intern(cls.py(), name).unbind()),
    })
}

fn read_default(default: &Bound<'_, PyAny>) -> PyResult<FieldDefault> {
    if default.is_none() {
        return Ok(FieldDefault::Required);
    }
    let kind: String = default.get_item(0)?.extract()?;
    let value = default.get_item(1)?.unbind();
    Ok(match kind.as_str() {
        "factory" => FieldDefault::Factory(value),
        _ => FieldDefault::Value(value),
    })
}

fn read_type(description: &Bound<'_, PyAny>) -> PyResult<TypeDescription> {
    let parts = description.cast::<PyTuple>()?;
    let kind: String = parts.get_item(0)?.extract()?;
    Ok(match kind.as_str() {
        "any" => TypeDescription::Any,
        "bool" => TypeDescription::Bool,
        "none" => TypeDescription::None,
        "int" => TypeDescription::Integer(read_constraints(&parts.get_item(1)?)?),
        "float" => TypeDescription::Float(read_constraints(&parts.get_item(1)?)?),
        "str" => TypeDescription::String(read_constraints(&parts.get_item(1)?)?),
        "bytes" => TypeDescription::Bytes {
            mutable: parts.get_item(1)?.extract()?,
            constraints: read_constraints(&parts.get_item(2)?)?,
        },
        "converted" => TypeDescription::Converted(parts.get_item(1)?.extract()?),
        "list" => TypeDescription::List {
            items: Box::new(read_type(&parts.get_item(1)?)?),
            constraints: read_constraints(&parts.get_item(2)?)?,
        },
        "collection" => TypeDescription::Collection {
            kind: match parts.get_item(1)?.extract::<String>()?.as_str() {
                "set" => SequenceKind::Set,
                "frozenset" => SequenceKind::FrozenSet,
                _ => SequenceKind::Tuple,
            },
            items: Box::new(read_type(&parts.get_item(2)?)?),
            constraints: read_constraints(&parts.get_item(3)?)?,
        },
        "tuple" => TypeDescription::Tuple(read_types(&parts.get_item(1)?)?),
        "choice" => TypeDescription::Choice(parts.get_item(1)?.extract()?),
        "dict" => TypeDescription::Dict {
            values: Box::new(read_type(&parts.get_item(1)?)?),
            constraints: read_constraints(&parts.get_item(2)?)?,
        },
        "optional" => TypeDescription::Optional(Box::new(read_type(&parts.get_item(1)?)?)),
        "struct" => TypeDescription::Struct(parts.get_item(1)?.extract()?),
        "union" => TypeDescription::Union(read_types(&parts.get_item(1)?)?),
        "untagged" => TypeDescription::Untagged(read_types(&parts.get_item(1)?)?),
        "tagged" => {
            let mode = match parts.get_item(1)?.extract::<String>()?.as_str() {
                "internal" => TagMode::Internal {
                    tag: parts.get_item(2)?.extract()?,
                },
                "adjacent" => TagMode::Adjacent {
                    tag: parts.get_item(2)?.extract()?,
                    content: parts.get_item(3)?.extract()?,
                },
                _ => TagMode::External,
            };
            TypeDescription::Tagged {
                mode,
                variants: read_types(&parts.get_item(4)?)?,
            }
        }
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown type kind {other:?}"
            )));
        }
    })
}

fn read_types(descriptions: &Bound<'_, PyAny>) -> PyResult<Vec<TypeDescription>> {
    descriptions
        .try_iter()?
        .map(|description| read_type(&description?))
        .collect()
}

fn read_constraints(values: &Bound<'_, PyAny>) -> PyResult<Constraints> {
    let values = values.cast::<PyDict>()?;
    let mut constraints = Constraints::default();
    for (key, value) in values.iter() {
        match key.extract::<String>()?.as_str() {
            "minimum" => constraints.minimum = Some(read_bound(&value)?),
            "maximum" => constraints.maximum = Some(read_bound(&value)?),
            "min_length" => constraints.min_length = Some(value.extract()?),
            "max_length" => constraints.max_length = Some(value.extract()?),
            "pattern" => constraints.pattern = Some(value.extract()?),
            "one_of" => {
                let literals = value
                    .try_iter()?
                    .map(|literal| {
                        let literal = literal?;
                        if literal.is_instance_of::<PyInt>() {
                            Ok(Literal::Integer(literal.extract()?))
                        } else {
                            Ok(Literal::String(literal.extract()?))
                        }
                    })
                    .collect::<PyResult<Vec<_>>>()?;
                constraints.one_of = Some(literals);
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown constraint {other:?}"
                )));
            }
        }
    }
    Ok(constraints)
}

fn read_bound(value: &Bound<'_, PyAny>) -> PyResult<NumericBound> {
    if value.is_instance_of::<PyInt>() {
        Ok(NumericBound::Integer(value.extract()?))
    } else {
        Ok(NumericBound::Float(value.extract()?))
    }
}
