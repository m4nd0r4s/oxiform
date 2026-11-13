//! Encoding as a declared type: the plan of `T` is walked, not the class of the value.
//!
//! The plan is read from `oxiform._introspection.declared_description` once per type and kept
//! for the life of the process, like the plans of decoding.

mod walk;

use std::collections::HashSet;
use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyInt, PyString, PyTuple, PyType};

use super::fields::{HintMode, Omission, read_omission};
use crate::compiled::compiled_for;
use crate::slots::slot_offset;

/// A declared type and the structures it reaches.
pub(crate) struct DeclaredPlan {
    root: Declared,
    structs: Vec<StructSpec>,
}

impl DeclaredPlan {
    pub(super) fn root(&self) -> &Declared {
        &self.root
    }
}

/// How a value declared with one type is written.
pub(super) enum Declared {
    /// Written by the class of the value.
    Any,
    /// A scalar, or a class without fields to filter: written by the class of the value.
    Leaf(Leaf),
    Literal {
        name: String,
        /// The values by kind, so that a check is a hash lookup: exact `str`s, exact `int`s
        /// that fit 64 bits, and the rest (`bool`, `None`, enum members, `bytes`, big ints).
        strings: HashSet<String>,
        integers: HashSet<i64>,
        others: Vec<Py<PyAny>>,
    },
    Optional(Box<Declared>),
    List(Box<Declared>),
    Dict(Box<Declared>),
    Collection {
        set: bool,
        items: Box<Declared>,
    },
    Tuple(Vec<Declared>),
    /// An index into the structures of the plan.
    Struct(usize),
    /// The variant is chosen by the class of the value.
    Union {
        name: String,
        variants: Vec<Declared>,
    },
    /// Each variant with the tag it is written with, when it has one.
    Tagged {
        name: String,
        mode: HintMode,
        variants: Vec<(Declared, Option<String>)>,
    },
    Untagged {
        name: String,
        variants: Vec<Declared>,
    },
}

pub(super) struct Leaf {
    name: String,
    /// The classes a value of a union variant is checked against.
    classes: Py<PyTuple>,
    /// Classes that do not count, although they are subclasses (`bool` for `int`).
    excluded: Option<Py<PyTuple>>,
    /// Classes accepted when no variant matches (`int` for `float`).
    lenient: Option<Py<PyTuple>>,
}

/// One field of a declared model.
pub(super) struct DeclaredField {
    name: Py<PyString>,
    wire_name: String,
    /// The slot offset, when the field is a plain slot of the declared class.
    offset: Option<isize>,
    flatten: bool,
    omission: Omission,
    /// A fixed value written instead of an attribute: the tag of the model.
    constant: Option<Py<PyString>>,
    declared: Declared,
}

/// The fields written for one declared model.
pub(super) struct StructSpec {
    name: String,
    cls: Py<PyType>,
    fields: Vec<DeclaredField>,
    /// Field indices in wire name order, for `sort_keys`.
    sorted: Vec<usize>,
    /// Every field is written under its own key, whatever its value.
    simple: bool,
}

/// Holds a declared plan in the cache dict.
#[pyclass(frozen, module = "oxiform._oxiform")]
pub(crate) struct DeclaredHandle {
    plan: Arc<DeclaredPlan>,
}

static PLANS: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
static DESCRIBE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

/// The plan of encoding as `target`, reading it on first use.
///
/// `target` must also be a type that decodes, so a declaration error is the same in both
/// directions.
pub(crate) fn declared_for(target: &Bound<'_, PyAny>) -> PyResult<Arc<DeclaredPlan>> {
    let py = target.py();
    let cache = PLANS.get_or_init(py, || PyDict::new(py).unbind()).bind(py);
    let cached = match cache.get_item(target) {
        Ok(cached) => cached,
        // Unhashable targets (Annotated with unhashable metadata) are read every time.
        Err(_) => return read_plan(target).map(Arc::new),
    };
    if let Some(handle) = cached {
        return Ok(handle.cast::<DeclaredHandle>()?.get().plan.clone());
    }
    let plan = Arc::new(read_plan(target)?);
    let handle = Bound::new(py, DeclaredHandle { plan: plan.clone() })?;
    cache.set_item(target, handle)?;
    Ok(plan)
}

fn read_plan(target: &Bound<'_, PyAny>) -> PyResult<DeclaredPlan> {
    let py = target.py();
    compiled_for(target)?;
    let description = DESCRIBE
        .import(py, "oxiform._introspection", "declared_description")?
        .call1((target,))?;
    let mut structs = Vec::new();
    for spec in description.getattr("structs")?.try_iter()? {
        structs.push(read_struct(&spec?)?);
    }
    Ok(DeclaredPlan {
        root: read_type(&description.getattr("root")?)?,
        structs,
    })
}

fn read_struct(spec: &Bound<'_, PyAny>) -> PyResult<StructSpec> {
    let py = spec.py();
    let cls = spec.getattr("cls")?.cast_into::<PyType>()?;
    let mut fields = Vec::new();
    for field in spec.getattr("fields")?.try_iter()? {
        let field = field?;
        let encoded = field.getattr("encoded")?;
        let name: String = encoded.getattr("name")?.extract()?;
        let constant = encoded.getattr("constant")?;
        fields.push(DeclaredField {
            offset: if constant.is_none() {
                slot_offset(&cls, &name)?
            } else {
                None
            },
            name: PyString::intern(py, &name).unbind(),
            wire_name: encoded.getattr("wire_name")?.extract()?,
            flatten: encoded.getattr("flatten")?.extract()?,
            omission: read_omission(&encoded)?,
            constant: (!constant.is_none())
                .then(|| constant.cast_into::<PyString>().map(Bound::unbind))
                .transpose()?,
            declared: read_type(&field.getattr("value_type")?)?,
        });
    }
    let mut sorted: Vec<usize> = (0..fields.len()).collect();
    sorted.sort_by(|&left, &right| fields[left].wire_name.cmp(&fields[right].wire_name));
    let simple = fields
        .iter()
        .all(|field| !field.flatten && field.omission.never() && field.constant.is_none());
    Ok(StructSpec {
        name: spec.getattr("name")?.extract()?,
        cls: cls.unbind(),
        fields,
        sorted,
        simple,
    })
}

fn read_type(description: &Bound<'_, PyAny>) -> PyResult<Declared> {
    let parts = description.cast::<PyTuple>()?;
    let kind = parts.get_item(0)?;
    let kind = kind.cast::<PyString>()?.to_str()?;
    let item = |index: usize| parts.get_item(index);
    let variants = |index: usize| -> PyResult<Vec<Declared>> {
        item(index)?
            .try_iter()?
            .map(|variant| read_type(&variant?))
            .collect()
    };
    Ok(match kind {
        "any" => Declared::Any,
        "leaf" => Declared::Leaf(Leaf {
            name: item(1)?.extract()?,
            classes: item(2)?.cast_into::<PyTuple>()?.unbind(),
            excluded: non_empty(&item(3)?)?,
            lenient: non_empty(&item(4)?)?,
        }),
        "literal" => read_literal(item(1)?.extract()?, &item(2)?)?,
        "optional" => Declared::Optional(Box::new(read_type(&item(1)?)?)),
        "list" => Declared::List(Box::new(read_type(&item(1)?)?)),
        "dict" => Declared::Dict(Box::new(read_type(&item(1)?)?)),
        "collection" => Declared::Collection {
            set: item(1)?.extract::<String>()? != "tuple",
            items: Box::new(read_type(&item(2)?)?),
        },
        "tuple" => Declared::Tuple(variants(1)?),
        "struct" => Declared::Struct(item(1)?.extract()?),
        "union" => Declared::Union {
            name: item(1)?.extract()?,
            variants: variants(2)?,
        },
        "untagged" => Declared::Untagged {
            name: item(1)?.extract()?,
            variants: variants(2)?,
        },
        "tagged" => Declared::Tagged {
            name: item(1)?.extract()?,
            mode: read_mode(&item(2)?, &item(3)?, &item(4)?)?,
            variants: item(5)?
                .try_iter()?
                .map(|pair| {
                    let pair = pair?;
                    let pair = pair.cast::<PyTuple>()?;
                    let tag = pair.get_item(1)?;
                    Ok((
                        read_type(&pair.get_item(0)?)?,
                        (!tag.is_none()).then(|| tag.extract()).transpose()?,
                    ))
                })
                .collect::<PyResult<_>>()?,
        },
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown declared type kind {other:?}"
            )));
        }
    })
}

fn read_literal(name: String, values: &Bound<'_, PyAny>) -> PyResult<Declared> {
    let mut strings = HashSet::new();
    let mut integers = HashSet::new();
    let mut others = Vec::new();
    for value in values.try_iter()? {
        let value = value?;
        if let Ok(text) = value.cast_exact::<PyString>() {
            strings.insert(text.to_str()?.to_owned());
        } else if let Some(number) = value
            .cast_exact::<PyInt>()
            .ok()
            .and_then(|number| number.extract::<i64>().ok())
        {
            integers.insert(number);
        } else {
            others.push(value.unbind());
        }
    }
    Ok(Declared::Literal {
        name,
        strings,
        integers,
        others,
    })
}

fn read_mode(
    mode: &Bound<'_, PyAny>,
    tag: &Bound<'_, PyAny>,
    content: &Bound<'_, PyAny>,
) -> PyResult<HintMode> {
    Ok(match mode.extract::<String>()?.as_str() {
        "internal" => HintMode::Internal(tag.extract()?),
        "adjacent" => HintMode::Adjacent(tag.extract()?, content.extract()?),
        _ => HintMode::External,
    })
}

fn non_empty(classes: &Bound<'_, PyAny>) -> PyResult<Option<Py<PyTuple>>> {
    let classes = classes.cast::<PyTuple>()?;
    Ok((!classes.is_empty()).then(|| classes.clone().unbind()))
}
