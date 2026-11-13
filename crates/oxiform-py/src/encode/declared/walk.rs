//! Writes a value as a declared type.

use oxiform_core::issues::PathSegment;
use oxiform_core::writer::EventWriter;
use pyo3::exceptions::{PyAttributeError, PyKeyError};
use pyo3::prelude::*;
use pyo3::types::{
    PyByteArray, PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PyMapping, PySet, PyString,
    PyTuple,
};

use super::{Declared, DeclaredField, DeclaredPlan, Leaf, StructSpec};
use crate::encode::fields::{HintMode, field_table};
use crate::encode::{EncodeFailure, ObjectWalker, key_text, sorted_items};
use crate::slots::read_slot;

/// A key, its value, and the declared type of the value.
type Entry<'p, 'py> = (&'p str, Bound<'py, PyAny>, &'p Declared);

/// Where the fields of a declared model are read from.
enum Source<'py> {
    /// An instance of the declared class, or of a subclass: its attributes.
    Instance,
    Dict(Bound<'py, PyDict>),
    Mapping(Bound<'py, PyMapping>),
    /// Any other object: its attributes.
    Object,
}

/// What a leaf of a union is checked against, from the most to the least exact.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Strictness {
    /// The class of the value is the declared class.
    Exact,
    /// The value is an instance of the declared class.
    Instance,
    /// The value is not an instance, but it can stand for the type (an `int` for a `float`, a
    /// mapping or an object that has every field of a model).
    Lenient,
}

static ANY: Declared = Declared::Any;

impl<W: EventWriter> ObjectWalker<'_, W> {
    /// Writes `object` as the root type of `plan`.
    pub(crate) fn declared_root(
        &mut self,
        plan: &DeclaredPlan,
        object: &Bound<'_, PyAny>,
    ) -> Result<(), EncodeFailure> {
        self.declared(plan, plan.root(), object)
    }

    fn declared(
        &mut self,
        plan: &DeclaredPlan,
        node: &Declared,
        object: &Bound<'_, PyAny>,
    ) -> Result<(), EncodeFailure> {
        match node {
            Declared::Any => self.value(object),
            Declared::Leaf(Leaf { name, .. }) | Declared::Literal { name, .. } => {
                self.scalar(name, object)
            }
            Declared::Optional(inner) => {
                if object.is_none() {
                    self.writer.null();
                    Ok(())
                } else {
                    self.declared(plan, inner, object)
                }
            }
            Declared::List(items) => self.declared_items(plan, "a list", items, object, false),
            Declared::Collection { set, items } => {
                self.declared_items(plan, "a sequence", items, object, *set)
            }
            Declared::Tuple(items) => self.declared_tuple(plan, items, object),
            Declared::Dict(values) => self.declared_dict(plan, values, object),
            Declared::Struct(index) => self.declared_struct(plan, *index, object, None),
            Declared::Union { name, variants } | Declared::Untagged { name, variants } => {
                let index = self.pick(plan, name, variants.iter(), object)?;
                self.declared(plan, &variants[index], object)
            }
            Declared::Tagged {
                name,
                mode,
                variants,
            } => {
                let index = self.pick(plan, name, variants.iter().map(|(v, _)| v), object)?;
                let (variant, tag) = &variants[index];
                match tag {
                    Some(tag) => self.declared_tagged(plan, mode, variant, tag, object),
                    None => self.declared(plan, variant, object),
                }
            }
        }
    }

    /// A value of a type without fields to filter. Writing it by its class is safe unless that
    /// class has fields of its own: a model, a mapping or a sequence is not what was declared.
    fn scalar(&mut self, name: &str, object: &Bound<'_, PyAny>) -> Result<(), EncodeFailure> {
        if is_composite(object)? {
            return Err(mismatch(
                object,
                name,
                "the declared type has no fields, so a model, a mapping or a sequence is not \
                 a value of it; return a value of the declared type, or declare the type of \
                 the model",
            ));
        }
        self.value(object)
    }

    fn declared_items(
        &mut self,
        plan: &DeclaredPlan,
        what: &str,
        items: &Declared,
        object: &Bound<'_, PyAny>,
        sets: bool,
    ) -> Result<(), EncodeFailure> {
        if let Ok(list) = object.cast::<PyList>() {
            self.declared_sequence(plan, items, list.len(), list.iter())
        } else if let Ok(tuple) = object.cast::<PyTuple>() {
            self.declared_sequence(plan, items, tuple.len(), tuple.iter())
        } else if sets && is_set(object) {
            let sorted = sorted_items(object)?;
            self.declared_sequence(plan, items, sorted.len(), sorted.iter())
        } else {
            Err(mismatch(
                object,
                what,
                "return a value of the declared type",
            ))
        }
    }

    fn declared_sequence<'py>(
        &mut self,
        plan: &DeclaredPlan,
        items: &Declared,
        length: usize,
        values: impl Iterator<Item = Bound<'py, PyAny>>,
    ) -> Result<(), EncodeFailure> {
        self.enter()?;
        self.writer.sequence_start(length);
        for (index, value) in values.enumerate() {
            self.declared(plan, items, &value)
                .map_err(|failure| failure.at(|| PathSegment::Index(index)))?;
        }
        self.writer.sequence_end();
        self.depth -= 1;
        Ok(())
    }

    fn declared_tuple(
        &mut self,
        plan: &DeclaredPlan,
        items: &[Declared],
        object: &Bound<'_, PyAny>,
    ) -> Result<(), EncodeFailure> {
        let values: Vec<Bound<'_, PyAny>> = if let Ok(tuple) = object.cast::<PyTuple>() {
            tuple.iter().collect()
        } else if let Ok(list) = object.cast::<PyList>() {
            list.iter().collect()
        } else {
            return Err(mismatch(
                object,
                "a tuple",
                "return a value of the declared type",
            ));
        };
        if values.len() != items.len() {
            let message = format!(
                "cannot encode a sequence of {} items as a tuple of {}; return a value with \
                 the declared number of items",
                values.len(),
                items.len()
            );
            return Err(EncodeFailure::Unsupported {
                message,
                path: Vec::new(),
            });
        }
        self.enter()?;
        self.writer.sequence_start(items.len());
        for (index, (item, value)) in items.iter().zip(&values).enumerate() {
            self.declared(plan, item, value)
                .map_err(|failure| failure.at(|| PathSegment::Index(index)))?;
        }
        self.writer.sequence_end();
        self.depth -= 1;
        Ok(())
    }

    fn declared_dict(
        &mut self,
        plan: &DeclaredPlan,
        values: &Declared,
        object: &Bound<'_, PyAny>,
    ) -> Result<(), EncodeFailure> {
        let mut entries = Vec::new();
        if let Ok(dict) = object.cast::<PyDict>() {
            for (key, value) in dict.iter() {
                entries.push((key_text(&key)?.into_owned(), value));
            }
        } else if let Ok(mapping) = object.cast::<PyMapping>() {
            for pair in mapping.items()?.try_iter()? {
                let pair = pair?;
                let pair = pair.cast::<PyTuple>().map_err(PyErr::from)?;
                entries.push((
                    key_text(&pair.get_item(0)?)?.into_owned(),
                    pair.get_item(1)?,
                ));
            }
        } else {
            return Err(mismatch(
                object,
                "a dict",
                "return a value of the declared type",
            ));
        }
        if self.sort_keys {
            entries.sort_by(|left, right| left.0.cmp(&right.0));
        }
        self.enter()?;
        self.writer.map_start(entries.len());
        for (key, value) in entries {
            self.writer.key(&key);
            self.declared(plan, values, &value)
                .map_err(|failure| failure.at(|| PathSegment::Key(key.clone())))?;
        }
        self.writer.map_end();
        self.depth -= 1;
        Ok(())
    }

    /// Writes the fields of the declared model `index` read from `object`, with `tag` (a key
    /// and its value) as the first entry when given.
    fn declared_struct(
        &mut self,
        plan: &DeclaredPlan,
        index: usize,
        object: &Bound<'_, PyAny>,
        tag: Option<(&str, &str)>,
    ) -> Result<(), EncodeFailure> {
        let spec = &plan.structs[index];
        let source =
            source_of(spec, object)?.ok_or_else(|| mismatch(object, &spec.name, STRUCT_ADVICE))?;
        self.enter()?;
        if spec.simple && tag.is_none() {
            self.writer.map_start(spec.fields.len());
            for position in 0..spec.fields.len() {
                let field = &spec.fields[if self.sort_keys {
                    spec.sorted[position]
                } else {
                    position
                }];
                let value = read_field(spec, field, object, &source)?;
                self.writer.key(&field.wire_name);
                self.declared(plan, &field.declared, &value)
                    .map_err(|failure| failure.at(|| PathSegment::Key(field.wire_name.clone())))?;
            }
        } else {
            let py = object.py();
            let mut entries: Vec<Entry<'_, '_>> = Vec::with_capacity(spec.fields.len() + 1);
            if let Some((key, text)) = tag {
                entries.push((key, PyString::new(py, text).into_any(), &ANY));
            }
            self.collect_declared(plan, spec, object, &source, &mut entries)?;
            if self.sort_keys {
                entries.sort_by(|left, right| left.0.cmp(right.0));
            }
            self.writer.map_start(entries.len());
            for (key, value, declared) in entries {
                self.writer.key(key);
                self.declared(plan, declared, &value)
                    .map_err(|failure| failure.at(|| PathSegment::Key(key.to_owned())))?;
            }
        }
        self.writer.map_end();
        self.depth -= 1;
        Ok(())
    }

    /// The entries of `object` that are written, with flattened models expanded in place.
    fn collect_declared<'p, 'py>(
        &mut self,
        plan: &'p DeclaredPlan,
        spec: &'p StructSpec,
        object: &Bound<'py, PyAny>,
        source: &Source<'py>,
        entries: &mut Vec<Entry<'p, 'py>>,
    ) -> Result<(), EncodeFailure> {
        let py = object.py();
        for field in &spec.fields {
            if let Some(constant) = &field.constant {
                let value = constant.bind(py).clone().into_any();
                entries.push((&field.wire_name, value, &ANY));
                continue;
            }
            let value = read_field(spec, field, object, source)?;
            if field.omission.applies(&value)? {
                continue;
            }
            let Declared::Struct(inner) = &field.declared else {
                entries.push((&field.wire_name, value, &field.declared));
                continue;
            };
            if !field.flatten {
                entries.push((&field.wire_name, value, &field.declared));
                continue;
            }
            let inner = &plan.structs[*inner];
            let inner_source = source_of(inner, &value)?.ok_or_else(|| {
                mismatch(&value, &inner.name, STRUCT_ADVICE)
                    .at(|| PathSegment::Key(field.wire_name.clone()))
            })?;
            self.enter()?;
            self.collect_declared(plan, inner, &value, &inner_source, entries)?;
            self.depth -= 1;
        }
        Ok(())
    }

    fn declared_tagged(
        &mut self,
        plan: &DeclaredPlan,
        mode: &HintMode,
        variant: &Declared,
        tag: &str,
        object: &Bound<'_, PyAny>,
    ) -> Result<(), EncodeFailure> {
        match mode {
            HintMode::Internal(field) => match variant {
                Declared::Struct(index) => {
                    self.declared_struct(plan, *index, object, Some((field, tag)))
                }
                _ => self.declared(plan, variant, object),
            },
            HintMode::Adjacent(tag_key, content) => {
                self.enter()?;
                self.writer.map_start(2);
                self.writer.key(tag_key);
                self.writer.string(tag);
                self.writer.key(content);
                self.declared(plan, variant, object)
                    .map_err(|failure| failure.at(|| PathSegment::Key(content.clone())))?;
                self.writer.map_end();
                self.depth -= 1;
                Ok(())
            }
            HintMode::External => {
                self.enter()?;
                self.writer.map_start(1);
                self.writer.key(tag);
                self.declared(plan, variant, object)
                    .map_err(|failure| failure.at(|| PathSegment::Key(tag.to_owned())))?;
                self.writer.map_end();
                self.depth -= 1;
                Ok(())
            }
        }
    }

    /// The variant that stands for `object`: the first, in declaration order, whose class is
    /// the class of the value; else the first the value is an instance of; else the first it
    /// can stand for.
    fn pick<'p>(
        &mut self,
        plan: &DeclaredPlan,
        name: &str,
        variants: impl Iterator<Item = &'p Declared> + Clone,
        object: &Bound<'_, PyAny>,
    ) -> Result<usize, EncodeFailure> {
        for strictness in [Strictness::Exact, Strictness::Instance, Strictness::Lenient] {
            for (index, variant) in variants.clone().enumerate() {
                if matches(plan, variant, object, strictness)? {
                    return Ok(index);
                }
            }
        }
        Err(mismatch(
            object,
            name,
            "the value is none of the declared variants; return an instance of one of them, or \
             declare a type that includes it",
        ))
    }
}

const STRUCT_ADVICE: &str = "expected an instance of the declared class, a mapping or an object \
                             that has its fields";

/// Whether `object` is a variant of its own: a model, a mapping or a sequence.
fn is_composite(object: &Bound<'_, PyAny>) -> PyResult<bool> {
    if object.is_none()
        || object.cast_exact::<PyString>().is_ok()
        || object.cast_exact::<PyInt>().is_ok()
        || object.cast_exact::<PyFloat>().is_ok()
    {
        return Ok(false);
    }
    Ok(object.is_instance_of::<PyDict>()
        || object.is_instance_of::<PyList>()
        || object.is_instance_of::<PyTuple>()
        || is_set(object)
        || field_table(&object.get_type())?.is_some())
}

fn is_set(object: &Bound<'_, PyAny>) -> bool {
    object.is_instance_of::<PySet>() || object.is_instance_of::<PyFrozenSet>()
}

/// Whether the value of a model is read from `object`: not for `None`, text, numbers and
/// sequences, which cannot hold fields.
fn can_hold_fields(object: &Bound<'_, PyAny>) -> bool {
    !(object.is_none()
        || object.is_instance_of::<PyString>()
        || object.is_instance_of::<PyInt>()
        || object.is_instance_of::<PyFloat>()
        || object.is_instance_of::<PyBytes>()
        || object.is_instance_of::<PyByteArray>()
        || object.is_instance_of::<PyList>()
        || object.is_instance_of::<PyTuple>()
        || is_set(object))
}

fn source_of<'py>(spec: &StructSpec, object: &Bound<'py, PyAny>) -> PyResult<Option<Source<'py>>> {
    let class = spec.cls.bind(object.py());
    if object.get_type().is(class) || object.is_instance(class)? {
        return Ok(Some(Source::Instance));
    }
    if let Ok(dict) = object.cast::<PyDict>() {
        return Ok(Some(Source::Dict(dict.clone())));
    }
    if let Ok(mapping) = object.cast::<PyMapping>() {
        return Ok(Some(Source::Mapping(mapping.clone())));
    }
    Ok(can_hold_fields(object).then_some(Source::Object))
}

/// The value of `field`, as an attribute or a key of `object`.
fn read_field<'py>(
    spec: &StructSpec,
    field: &DeclaredField,
    object: &Bound<'py, PyAny>,
    source: &Source<'py>,
) -> Result<Bound<'py, PyAny>, EncodeFailure> {
    let py = object.py();
    let name = field.name.bind(py);
    let found = match source {
        Source::Instance => {
            let slot = field.offset.and_then(|offset| {
                // SAFETY: the offset was read for the declared class, and `object` is an
                // instance of it or of a subclass, which keeps the layout of its bases.
                unsafe { read_slot(object, offset) }
            });
            match slot {
                Some(value) => Some(value),
                None => attribute(object, name)?,
            }
        }
        Source::Dict(dict) => dict.get_item(name)?,
        Source::Mapping(mapping) => match mapping.get_item(name) {
            Ok(value) => Some(value),
            Err(error) if error.is_instance_of::<PyKeyError>(py) => None,
            Err(error) => return Err(error.into()),
        },
        Source::Object => attribute(object, name)?,
    };
    found.ok_or_else(|| {
        let (container, lookup) = match source {
            Source::Dict(_) | Source::Mapping(_) => ("the mapping", "key"),
            _ => ("the object", "attribute"),
        };
        let message = format!(
            "cannot encode {} as {}: {container} has no {lookup} {:?}; add it, or declare a \
             type without that field (a declared model is read as attributes from an object \
             and as keys from a mapping)",
            type_name(object),
            spec.name,
            field.name.bind(py).to_string()
        );
        EncodeFailure::Unsupported {
            message,
            path: Vec::new(),
        }
    })
}

fn attribute<'py>(
    object: &Bound<'py, PyAny>,
    name: &Bound<'py, PyString>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    match object.getattr(name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_instance_of::<PyAttributeError>(object.py()) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether `object` can stand for the declared type `node` at the given strictness.
fn matches(
    plan: &DeclaredPlan,
    node: &Declared,
    object: &Bound<'_, PyAny>,
    strictness: Strictness,
) -> PyResult<bool> {
    let py = object.py();
    Ok(match node {
        Declared::Leaf(leaf) => match strictness {
            Strictness::Exact => {
                let object_type = object.get_type();
                leaf.classes
                    .bind(py)
                    .iter()
                    .any(|class| class.is(&object_type))
            }
            Strictness::Instance => {
                object.is_instance(leaf.classes.bind(py))?
                    && match &leaf.excluded {
                        Some(excluded) => !object.is_instance(excluded.bind(py))?,
                        None => true,
                    }
            }
            Strictness::Lenient => match &leaf.lenient {
                Some(lenient) => {
                    object.is_instance(lenient.bind(py))?
                        && !object.is_instance_of::<pyo3::types::PyBool>()
                }
                None => false,
            },
        },
        Declared::Literal { values, .. } => {
            if strictness == Strictness::Lenient {
                return Ok(false);
            }
            let object_type = object.get_type();
            for value in values {
                let value = value.bind(py);
                if value.get_type().is(&object_type) && object.eq(value)? {
                    return Ok(true);
                }
            }
            false
        }
        Declared::Struct(index) => {
            let spec = &plan.structs[*index];
            match strictness {
                Strictness::Exact => object.get_type().is(spec.cls.bind(py)),
                Strictness::Instance => object.is_instance(spec.cls.bind(py))?,
                Strictness::Lenient => match source_of(spec, object)? {
                    Some(source) => has_every_field(spec, object, &source)?,
                    None => false,
                },
            }
        }
        Declared::List(_) => {
            strictness == Strictness::Instance && object.is_instance_of::<PyList>()
        }
        Declared::Collection { set, .. } => {
            strictness == Strictness::Instance
                && if *set {
                    is_set(object)
                } else {
                    object.is_instance_of::<PyTuple>()
                }
        }
        Declared::Tuple(items) => {
            strictness == Strictness::Instance
                && object
                    .cast::<PyTuple>()
                    .is_ok_and(|tuple| tuple.len() == items.len())
        }
        Declared::Dict(_) => {
            strictness == Strictness::Instance
                && (object.is_instance_of::<PyDict>() || object.is_instance_of::<PyMapping>())
        }
        // These are not variants of a union.
        Declared::Any
        | Declared::Optional(_)
        | Declared::Union { .. }
        | Declared::Tagged { .. }
        | Declared::Untagged { .. } => false,
    })
}

/// Whether `object` has every field of `spec` that is read from it (the tag is not read).
fn has_every_field(
    spec: &StructSpec,
    object: &Bound<'_, PyAny>,
    source: &Source<'_>,
) -> PyResult<bool> {
    let py = object.py();
    for field in spec.fields.iter().filter(|field| field.constant.is_none()) {
        let name = field.name.bind(py);
        let present = match source {
            Source::Dict(dict) => dict.contains(name)?,
            Source::Mapping(mapping) => mapping.contains(name)?,
            Source::Instance | Source::Object => object.hasattr(name)?,
        };
        if !present {
            return Ok(false);
        }
    }
    Ok(true)
}

fn type_name(object: &Bound<'_, PyAny>) -> String {
    object
        .get_type()
        .qualname()
        .map_or_else(|_| "an object".to_owned(), |name| name.to_string())
}

/// `object` cannot be written as the declared type `declared`.
fn mismatch(object: &Bound<'_, PyAny>, declared: &str, advice: &str) -> EncodeFailure {
    EncodeFailure::Unsupported {
        message: format!(
            "cannot encode {} as {declared}: {advice}",
            type_name(object)
        ),
        path: Vec::new(),
    }
}
