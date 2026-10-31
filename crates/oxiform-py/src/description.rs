//! Reads the description built by `oxiform._introspection` into the Rust description, keeping
//! what the sink needs to create instances.

use oxiform_core::description::{
    Bound as NumericBound, Constraints, Description, FieldDescription, Literal, StructDescription,
    TypeDescription, UnknownFields,
};
use pyo3::exceptions::PyValueError;
use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyString, PyTuple, PyType};

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

/// How to create instances of one structure.
pub(crate) struct StructRuntime {
    pub(crate) cls: Py<PyType>,
    /// In field order.
    pub(crate) setters: Vec<FieldSetter>,
    pub(crate) defaults: Vec<FieldDefault>,
    pub(crate) has_post_init: bool,
}

/// Converts a Python `Description`.
pub(crate) fn read_description(
    description: &Bound<'_, PyAny>,
) -> PyResult<(Description, Vec<StructRuntime>)> {
    let root = read_type(&description.getattr("root")?)?;
    let mut structs = Vec::new();
    let mut runtimes = Vec::new();
    for struct_description in description.getattr("structs")?.try_iter()? {
        let (struct_description, runtime) = read_struct(&struct_description?)?;
        structs.push(struct_description);
        runtimes.push(runtime);
    }
    Ok((Description { root, structs }, runtimes))
}

fn read_struct(description: &Bound<'_, PyAny>) -> PyResult<(StructDescription, StructRuntime)> {
    let mut fields = Vec::new();
    let cls = description.getattr("cls")?.cast_into::<PyType>()?;
    let mut setters = Vec::new();
    let mut defaults = Vec::new();
    for field in description.getattr("fields")?.try_iter()? {
        let field = field?;
        let name: String = field.getattr("name")?.extract()?;
        let default = read_default(&field.getattr("default")?)?;
        fields.push(FieldDescription {
            wire_name: field.getattr("wire_name")?.extract()?,
            value_type: read_type(&field.getattr("value_type")?)?,
            has_default: !matches!(default, FieldDefault::Required),
            name: name.clone(),
        });
        setters.push(field_setter(&cls, &name)?);
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
    };
    let runtime = StructRuntime {
        cls: cls.unbind(),
        setters,
        defaults,
        has_post_init: description.getattr("has_post_init")?.extract()?,
    };
    Ok((struct_description, runtime))
}

/// Finds the `__slots__` member of `name` along the MRO of `cls`, if there is one.
fn field_setter(cls: &Bound<'_, PyType>, name: &str) -> PyResult<FieldSetter> {
    let py = cls.py();
    let member_type = py.import("types")?.getattr("MemberDescriptorType")?;
    for klass in cls.getattr("__mro__")?.try_iter()? {
        let Some(descriptor) = klass?.getattr("__dict__")?.get_item(name).ok() else {
            continue;
        };
        if !descriptor.get_type().is(&member_type) {
            break;
        }
        // SAFETY: `descriptor` is a member descriptor, whose C layout is
        // `PyMemberDescrObject`; it stays alive as long as the class, which the plan keeps.
        let member =
            unsafe { &*(*descriptor.as_ptr().cast::<ffi::PyMemberDescrObject>()).d_member };
        let writable = member.flags & ffi::Py_READONLY == 0;
        if member.type_code == ffi::Py_T_OBJECT_EX && writable && member.offset > 0 {
            return Ok(FieldSetter::Slot(member.offset));
        }
        break;
    }
    Ok(FieldSetter::Attribute(PyString::intern(py, name).unbind()))
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
        "list" => TypeDescription::List {
            items: Box::new(read_type(&parts.get_item(1)?)?),
            constraints: read_constraints(&parts.get_item(2)?)?,
        },
        "dict" => TypeDescription::Dict {
            values: Box::new(read_type(&parts.get_item(1)?)?),
            constraints: read_constraints(&parts.get_item(2)?)?,
        },
        "optional" => TypeDescription::Optional(Box::new(read_type(&parts.get_item(1)?)?)),
        "struct" => TypeDescription::Struct(parts.get_item(1)?.extract()?),
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown type kind {other:?}"
            )));
        }
    })
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
