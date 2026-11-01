//! Walks Python objects by their runtime type and hands them to a format writer.

mod fields;
mod hints;
pub(crate) mod json;
mod temporal;

use std::borrow::Cow;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use oxiform_core::issues::PathSegment;
use oxiform_core::writer::{EventWriter, WriteError};
use pyo3::exceptions::PyTypeError;
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PySet, PyString,
    PyTuple, PyType,
};

use crate::python_objects::encode_error;

/// A key, its value, and what the declared type adds to the value.
type Entry<'py> = (String, Bound<'py, PyAny>, Option<Arc<fields::EncodeHint>>);

/// Nesting deeper than this is refused: it is almost always a reference cycle.
const MAX_DEPTH: usize = 128;

/// Why an object could not be written.
pub(crate) enum EncodeFailure {
    /// Python code raised.
    Python(PyErr),
    /// The object, at `path`, has no representation in the format.
    Unsupported {
        message: String,
        path: Vec<PathSegment>,
    },
}

impl From<PyErr> for EncodeFailure {
    fn from(error: PyErr) -> Self {
        Self::Python(error)
    }
}

impl From<WriteError> for EncodeFailure {
    fn from(error: WriteError) -> Self {
        Self::Unsupported {
            message: error.message,
            path: Vec::new(),
        }
    }
}

impl EncodeFailure {
    /// The exception to raise: `ox.EncodeError` with the location, or the Python error.
    pub(crate) fn into_error(self, py: Python<'_>) -> PyErr {
        match self {
            Self::Python(error) => error,
            Self::Unsupported { message, path } => {
                let pointer = pointer(&path);
                let full = if pointer.is_empty() {
                    message
                } else {
                    format!("{message} at {pointer}")
                };
                match encode_error(py) {
                    Ok(class) => PyErr::from_type(class.clone(), (full, pointer)),
                    Err(error) => error,
                }
            }
        }
    }

    fn at(mut self, segment: impl FnOnce() -> PathSegment) -> Self {
        if let Self::Unsupported { path, .. } = &mut self {
            path.insert(0, segment());
        }
        self
    }
}

fn pointer(path: &[PathSegment]) -> String {
    let issue = oxiform_core::issues::Issue {
        code: oxiform_core::issues::ErrorCode::Type,
        path: path.to_vec(),
        message: String::new(),
        input: None,
        context: Vec::new(),
    };
    issue.pointer()
}

/// Writes Python objects with `writer`.
pub(crate) struct ObjectWalker<'writer, W> {
    pub(crate) writer: &'writer mut W,
    pub(crate) sort_keys: bool,
    depth: usize,
}

impl<'writer, W: EventWriter> ObjectWalker<'writer, W> {
    pub(crate) fn new(writer: &'writer mut W, sort_keys: bool) -> Self {
        Self {
            writer,
            sort_keys,
            depth: 0,
        }
    }

    pub(crate) fn value(&mut self, object: &Bound<'_, PyAny>) -> Result<(), EncodeFailure> {
        if object.is_none() {
            self.writer.null();
        } else if let Ok(text) = object.cast_exact::<PyString>() {
            self.writer.string(text.to_str()?);
        } else if let Ok(integer) = object.cast_exact::<PyInt>() {
            self.integer(integer)?;
        } else if let Ok(number) = object.cast_exact::<PyFloat>() {
            self.writer.float(number.value())?;
        } else if let Ok(list) = object.cast_exact::<PyList>() {
            self.sequence(list.len(), list.iter())?;
        } else if let Ok(dict) = object.cast_exact::<PyDict>() {
            self.mapping(dict)?;
        } else {
            self.other(object)?;
        }
        Ok(())
    }

    /// Subclasses of builtins (`IntEnum`, `StrEnum`), tuples, booleans, dataclasses, and the
    /// types written as text.
    fn other(&mut self, object: &Bound<'_, PyAny>) -> Result<(), EncodeFailure> {
        if let Ok(flag) = object.cast::<PyBool>() {
            self.writer.bool(flag.is_true());
        } else if let Ok(integer) = object.cast::<PyInt>() {
            self.integer(integer)?;
        } else if let Ok(text) = object.cast::<PyString>() {
            self.writer.string(text.to_str()?);
        } else if let Ok(number) = object.cast::<PyFloat>() {
            self.writer.float(number.value())?;
        } else if let Ok(tuple) = object.cast::<PyTuple>() {
            self.sequence(tuple.len(), tuple.iter())?;
        } else if let Ok(list) = object.cast::<PyList>() {
            self.sequence(list.len(), list.iter())?;
        } else if let Ok(dict) = object.cast::<PyDict>() {
            self.mapping(dict)?;
        } else if let Some(table) = fields::field_table(&object.get_type())? {
            self.structure(object, table.get())?;
        } else if object.is_instance_of::<PySet>() || object.is_instance_of::<PyFrozenSet>() {
            self.set(object)?;
        } else if object.is_instance(enum_type(object.py())?)? {
            self.value(&object.getattr(intern!(object.py(), "value"))?)?;
        } else if let Some(value) = temporal::temporal_of(object)? {
            let mut text = String::with_capacity(32);
            value.write_iso(&mut text);
            self.writer.string(&text);
        } else if let Ok(bytes) = object.cast::<PyBytes>() {
            self.writer.string(&STANDARD.encode(bytes.as_bytes()));
        } else if let Ok(bytes) = object.cast::<PyByteArray>() {
            self.writer.string(&STANDARD.encode(bytes.to_vec()));
        } else if object.is_instance(text_types(object.py())?)? {
            self.writer.string(object.str()?.to_str()?);
        } else if object.is_instance(zone_info_type(object.py())?)? {
            self.zone_info(object)?;
        } else {
            let type_name = object.get_type().qualname()?;
            return Err(EncodeFailure::Unsupported {
                message: format!("cannot encode an object of type {type_name}"),
                path: Vec::new(),
            });
        }
        Ok(())
    }

    /// A set or frozenset, sorted when its items can be compared, so the same set is always
    /// written the same way.
    fn set(&mut self, set: &Bound<'_, PyAny>) -> Result<(), EncodeFailure> {
        let py = set.py();
        let items = match sorted(py)?.call1((set,)) {
            Ok(items) => items.cast_into::<PyList>().map_err(PyErr::from)?,
            Err(error) if error.is_instance_of::<PyTypeError>(py) => {
                PyList::new(py, set.try_iter()?.collect::<PyResult<Vec<_>>>()?)?
            }
            Err(error) => return Err(error.into()),
        };
        self.sequence(items.len(), items.iter())
    }

    /// A time zone, by its key. A zone read from a file has none.
    fn zone_info(&mut self, zone: &Bound<'_, PyAny>) -> Result<(), EncodeFailure> {
        let key = zone.getattr(intern!(zone.py(), "key"))?;
        if key.is_none() {
            return Err(EncodeFailure::Unsupported {
                message: "cannot encode a ZoneInfo without a key".into(),
                path: Vec::new(),
            });
        }
        self.writer
            .string(key.cast::<PyString>().map_err(PyErr::from)?.to_str()?);
        Ok(())
    }

    fn integer(&mut self, integer: &Bound<'_, PyInt>) -> Result<(), EncodeFailure> {
        match integer.extract::<i64>() {
            Ok(value) => self.writer.integer(value),
            Err(_) => self.writer.big_integer(integer.str()?.to_str()?)?,
        }
        Ok(())
    }

    fn enter(&mut self) -> Result<(), EncodeFailure> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(EncodeFailure::Unsupported {
                message: format!(
                    "nesting deeper than {MAX_DEPTH} levels (is there a reference cycle?)"
                ),
                path: Vec::new(),
            });
        }
        Ok(())
    }

    fn sequence<'py>(
        &mut self,
        length: usize,
        items: impl Iterator<Item = Bound<'py, PyAny>>,
    ) -> Result<(), EncodeFailure> {
        self.enter()?;
        self.writer.sequence_start(length);
        for (index, item) in items.enumerate() {
            self.value(&item)
                .map_err(|failure| failure.at(|| PathSegment::Index(index)))?;
        }
        self.writer.sequence_end();
        self.depth -= 1;
        Ok(())
    }

    fn mapping(&mut self, dict: &Bound<'_, PyDict>) -> Result<(), EncodeFailure> {
        self.enter()?;
        self.writer.map_start(dict.len());
        if self.sort_keys {
            let mut entries = Vec::with_capacity(dict.len());
            for (key, value) in dict.iter() {
                entries.push((key_text(&key)?.into_owned(), value));
            }
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, value) in entries {
                self.entry(&key, &value)?;
            }
        } else {
            for (key, value) in dict.iter() {
                self.entry(&key_text(&key)?, &value)?;
            }
        }
        self.writer.map_end();
        self.depth -= 1;
        Ok(())
    }

    fn structure(
        &mut self,
        object: &Bound<'_, PyAny>,
        table: &fields::FieldTable,
    ) -> Result<(), EncodeFailure> {
        self.tagged_structure(object, table, None)
    }

    /// Writes a model, with `tag` (a key and its value) as its first entry when given.
    fn tagged_structure<'py>(
        &mut self,
        object: &Bound<'py, PyAny>,
        table: &fields::FieldTable,
        tag: Option<(&str, Bound<'py, PyAny>)>,
    ) -> Result<(), EncodeFailure> {
        self.enter()?;
        if table.is_simple() && tag.is_none() {
            self.writer.map_start(table.len());
            for position in 0..table.len() {
                let index = table.index(position, self.sort_keys);
                let value = table.read(object, index)?;
                self.entry(table.wire_name(object.py(), index).to_str()?, &value)?;
            }
        } else {
            let mut entries = Vec::with_capacity(table.len() + 1);
            if let Some((key, value)) = tag {
                entries.push((key.to_owned(), value, None));
            }
            self.collect_entries(object, table, &mut entries)?;
            if self.sort_keys {
                entries.sort_by(|left, right| left.0.cmp(&right.0));
            }
            self.writer.map_start(entries.len());
            for (key, value, hint) in entries {
                self.writer.key(&key);
                let written = match &hint {
                    Some(hint) => self.hinted(&value, hint),
                    None => self.value(&value),
                };
                written.map_err(|failure| failure.at(|| PathSegment::Key(key.clone())))?;
            }
        }
        self.writer.map_end();
        self.depth -= 1;
        Ok(())
    }

    /// The entries of `object` that are written, with flattened models expanded in place.
    fn collect_entries<'py>(
        &mut self,
        object: &Bound<'py, PyAny>,
        table: &fields::FieldTable,
        entries: &mut Vec<Entry<'py>>,
    ) -> Result<(), EncodeFailure> {
        let py = object.py();
        for index in 0..table.len() {
            let field = table.field(index);
            let key = table.wire_name(py, index).to_str()?;
            if let Some(constant) = &field.constant {
                entries.push((key.to_owned(), constant.bind(py).clone().into_any(), None));
                continue;
            }
            let value = table.read(object, index)?;
            if field.omission.applies(&value)? {
                continue;
            }
            if !field.flatten {
                entries.push((key.to_owned(), value, field.hint.clone()));
                continue;
            }
            let Some(inner) = fields::field_table(&value.get_type())? else {
                let type_name = value.get_type().qualname()?;
                return Err(EncodeFailure::Unsupported {
                    message: format!("a flattened field holds a model, not {type_name}"),
                    path: vec![PathSegment::Key(key.to_owned())],
                });
            };
            self.enter()?;
            self.collect_entries(&value, inner.get(), entries)?;
            self.depth -= 1;
        }
        Ok(())
    }

    fn entry(&mut self, key: &str, value: &Bound<'_, PyAny>) -> Result<(), EncodeFailure> {
        self.writer.key(key);
        self.value(value)
            .map_err(|failure| failure.at(|| PathSegment::Key(key.to_owned())))
    }
}

static ENUM: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static SORTED: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
static TEXT_TYPES: PyOnceLock<Py<PyTuple>> = PyOnceLock::new();
static ZONE_INFO: PyOnceLock<Py<PyType>> = PyOnceLock::new();

fn enum_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    ENUM.import(py, "enum", "Enum")
}

/// The types written as `str(value)`: `Decimal`, `UUID`, paths and IP addresses.
fn text_types(py: Python<'_>) -> PyResult<&Bound<'_, PyTuple>> {
    TEXT_TYPES.import(py, "oxiform._introspection.conversions", "TEXT_TYPES")
}

fn zone_info_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    ZONE_INFO.import(py, "zoneinfo", "ZoneInfo")
}

fn sorted(py: Python<'_>) -> PyResult<&Bound<'_, PyAny>> {
    SORTED.import(py, "builtins", "sorted")
}

/// A dict key as text: `str` keys as they are, `int` keys in decimal.
fn key_text<'key>(key: &'key Bound<'_, PyAny>) -> Result<Cow<'key, str>, EncodeFailure> {
    if let Ok(text) = key.cast::<PyString>() {
        return Ok(Cow::Borrowed(text.to_str()?));
    }
    if key.is_instance_of::<PyInt>() && !key.is_instance_of::<PyBool>() {
        return Ok(Cow::Owned(key.str()?.to_str()?.to_owned()));
    }
    let type_name = key.get_type().qualname()?;
    Err(EncodeFailure::Unsupported {
        message: format!("dict keys must be str or int, not {type_name}"),
        path: Vec::new(),
    })
}
