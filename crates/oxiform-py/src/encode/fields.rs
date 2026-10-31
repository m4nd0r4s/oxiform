//! The fields of dataclasses, looked up once per class.

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyString, PyTuple, PyType};

use crate::slots::{read_slot, slot_offset};

/// When a field is left out of the output.
pub(super) struct Omission {
    if_none: bool,
    /// The default value, or a factory that makes it.
    if_default: Option<Default>,
    if_predicate: Option<Py<PyAny>>,
}

enum Default {
    Value(Py<PyAny>),
    Factory(Py<PyAny>),
}

impl Omission {
    fn never(&self) -> bool {
        !self.if_none && self.if_default.is_none() && self.if_predicate.is_none()
    }

    /// Whether `value` is left out.
    pub(super) fn applies(&self, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let py = value.py();
        if self.if_none && value.is_none() {
            return Ok(true);
        }
        match &self.if_default {
            Some(Default::Value(default)) if value.eq(default.bind(py))? => return Ok(true),
            Some(Default::Factory(factory)) if value.eq(factory.bind(py).call0()?)? => {
                return Ok(true);
            }
            _ => {}
        }
        match &self.if_predicate {
            Some(predicate) => predicate.bind(py).call1((value,))?.is_truthy(),
            None => Ok(false),
        }
    }
}

/// One field that is written.
pub(super) struct TableField {
    name: Py<PyString>,
    wire_name: Py<PyString>,
    /// The slot offset, when the field is a plain slot.
    offset: Option<isize>,
    pub(super) flatten: bool,
    pub(super) omission: Omission,
}

/// How to write the fields of one dataclass.
#[pyclass(frozen, module = "oxiform._oxiform")]
pub(crate) struct FieldTable {
    fields: Vec<TableField>,
    /// Field indices in wire name order, for `sort_keys`.
    sorted: Vec<usize>,
    /// Every field is written under its own key, whatever its value.
    simple: bool,
}

impl FieldTable {
    pub(super) fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether every field is always written under its own key, so the output has exactly
    /// [`len`](Self::len) entries in table order.
    pub(super) fn is_simple(&self) -> bool {
        self.simple
    }

    /// The field at `position` in declaration order, or in wire name order when `sorted`.
    pub(super) fn index(&self, position: usize, sorted: bool) -> usize {
        if sorted {
            self.sorted[position]
        } else {
            position
        }
    }

    pub(super) fn field(&self, index: usize) -> &TableField {
        &self.fields[index]
    }

    pub(super) fn wire_name<'py>(&self, py: Python<'py>, index: usize) -> &Bound<'py, PyString> {
        self.fields[index].wire_name.bind(py)
    }

    /// The value of field `index` of `object`, an instance of the class of this table.
    pub(super) fn read<'py>(
        &self,
        object: &Bound<'py, PyAny>,
        index: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let field = &self.fields[index];
        if let Some(offset) = field.offset {
            // SAFETY: the offset was read for this exact class, the type of `object`.
            if let Some(value) = unsafe { read_slot(object, offset) } {
                return Ok(value);
            }
        }
        object.getattr(field.name.bind(object.py()))
    }
}

static TABLES: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
static ENCODED_FIELDS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

/// The field table of `cls`, or `None` when it is not a dataclass.
pub(super) fn field_table<'py>(
    cls: &Bound<'py, PyType>,
) -> PyResult<Option<Bound<'py, FieldTable>>> {
    let py = cls.py();
    let cache = TABLES.get_or_init(py, || PyDict::new(py).unbind()).bind(py);
    if let Some(cached) = cache.get_item(cls)? {
        return Ok(cached.cast_into::<FieldTable>().ok());
    }
    if !cls.hasattr("__dataclass_fields__")? {
        cache.set_item(cls, py.None())?;
        return Ok(None);
    }
    let mut fields = Vec::new();
    for field in ENCODED_FIELDS
        .import(py, "oxiform._introspection", "encoded_fields")?
        .call1((cls,))?
        .try_iter()?
    {
        fields.push(read_field(cls, &field?)?);
    }
    let mut sorted: Vec<usize> = (0..fields.len()).collect();
    sorted.sort_by_key(|&index| fields[index].wire_name.bind(py).to_string());
    let simple = fields
        .iter()
        .all(|field| !field.flatten && field.omission.never());
    let table = Bound::new(
        py,
        FieldTable {
            fields,
            sorted,
            simple,
        },
    )?;
    cache.set_item(cls, &table)?;
    Ok(Some(table))
}

fn read_field(cls: &Bound<'_, PyType>, field: &Bound<'_, PyAny>) -> PyResult<TableField> {
    let py = cls.py();
    let name: String = field.getattr("name")?.extract()?;
    let wire_name: String = field.getattr("wire_name")?.extract()?;
    let if_default = field.getattr("omit_if_default")?;
    let if_default = if if_default.is_none() {
        None
    } else {
        let default = if_default.cast::<PyTuple>()?;
        let value = default.get_item(1)?.unbind();
        Some(match default.get_item(0)?.extract::<String>()?.as_str() {
            "factory" => Default::Factory(value),
            _ => Default::Value(value),
        })
    };
    let if_predicate = field.getattr("omit_if")?;
    Ok(TableField {
        offset: slot_offset(cls, &name)?,
        name: PyString::intern(py, &name).unbind(),
        wire_name: PyString::intern(py, &wire_name).unbind(),
        flatten: field.getattr("flatten")?.extract()?,
        omission: Omission {
            if_none: field.getattr("omit_if_none")?.extract()?,
            if_default,
            if_predicate: (!if_predicate.is_none()).then(|| if_predicate.unbind()),
        },
    })
}
