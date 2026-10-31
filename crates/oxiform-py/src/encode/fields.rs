//! The fields of dataclasses, looked up once per class.

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyString, PyType};

use crate::slots::{read_slot, slot_offset};

/// How to read the fields of one dataclass.
#[pyclass(frozen, module = "oxiform._oxiform")]
pub(crate) struct FieldTable {
    /// Interned names, in declaration order.
    names: Vec<Py<PyString>>,
    /// The slot offset of each field, when it is a plain slot.
    offsets: Vec<Option<isize>>,
    /// Field indices in name order, for `sort_keys`.
    sorted: Vec<usize>,
}

impl FieldTable {
    pub(super) fn len(&self) -> usize {
        self.names.len()
    }

    /// The field at `position` in declaration order, or in name order when `sorted`.
    pub(super) fn index(&self, position: usize, sorted: bool) -> usize {
        if sorted {
            self.sorted[position]
        } else {
            position
        }
    }

    pub(super) fn name<'py>(&self, py: Python<'py>, index: usize) -> &Bound<'py, PyString> {
        self.names[index].bind(py)
    }

    /// The value of field `index` of `object`, an instance of the class of this table.
    pub(super) fn read<'py>(
        &self,
        object: &Bound<'py, PyAny>,
        index: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        if let Some(offset) = self.offsets[index] {
            // SAFETY: the offset was read for this exact class, the type of `object`.
            if let Some(value) = unsafe { read_slot(object, offset) } {
                return Ok(value);
            }
        }
        object.getattr(self.names[index].bind(object.py()))
    }
}

static TABLES: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
static DATACLASS_FIELDS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

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
    let mut names = Vec::new();
    let mut offsets = Vec::new();
    for field in DATACLASS_FIELDS
        .import(py, "dataclasses", "fields")?
        .call1((cls,))?
        .try_iter()?
    {
        let name: String = field?.getattr("name")?.extract()?;
        offsets.push(slot_offset(cls, &name)?);
        names.push(PyString::intern(py, &name).unbind());
    }
    let mut sorted: Vec<usize> = (0..names.len()).collect();
    sorted.sort_by_key(|&index| names[index].bind(py).to_string());
    let table = Bound::new(
        py,
        FieldTable {
            names,
            offsets,
            sorted,
        },
    )?;
    cache.set_item(cls, &table)?;
    Ok(Some(table))
}
