//! Values whose declared type says more than their runtime type: the tag of a tagged union.

use oxiform_core::issues::PathSegment;
use oxiform_core::writer::EventWriter;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use super::fields::{EncodeHint, HintMode, field_table};
use super::{EncodeFailure, ObjectWalker, key_text};

impl<W: EventWriter> ObjectWalker<'_, W> {
    /// Writes `object`, declared with the type that `hint` describes.
    pub(super) fn hinted(
        &mut self,
        object: &Bound<'_, PyAny>,
        hint: &EncodeHint,
    ) -> Result<(), EncodeFailure> {
        match hint {
            EncodeHint::List(items) => match object.cast::<PyList>() {
                Ok(list) => self.hinted_list(list, items),
                Err(_) => self.value(object),
            },
            EncodeHint::Dict(values) => match object.cast::<PyDict>() {
                Ok(dict) => self.hinted_dict(dict, values),
                Err(_) => self.value(object),
            },
            EncodeHint::Tagged { mode, tags } => {
                let py = object.py();
                let Some(tag) = tags.bind(py).get_item(object.get_type())? else {
                    return self.value(object);
                };
                let tag = tag.cast_into::<PyString>().map_err(PyErr::from)?;
                self.tagged(object, mode, &tag)
            }
        }
    }

    fn tagged(
        &mut self,
        object: &Bound<'_, PyAny>,
        mode: &HintMode,
        tag: &Bound<'_, PyString>,
    ) -> Result<(), EncodeFailure> {
        match mode {
            HintMode::Internal(field) => {
                let table =
                    field_table(&object.get_type())?.expect("only dataclasses are given a tag");
                self.tagged_structure(object, table.get(), Some((field, tag.clone().into_any())))
            }
            HintMode::Adjacent(tag_key, content) => {
                self.enter()?;
                self.writer.map_start(2);
                self.writer.key(tag_key);
                self.writer.string(tag.to_str()?);
                self.writer.key(content);
                self.value(object)
                    .map_err(|failure| failure.at(|| PathSegment::Key(content.clone())))?;
                self.writer.map_end();
                self.depth -= 1;
                Ok(())
            }
            HintMode::External => {
                let key = tag.to_str()?;
                self.enter()?;
                self.writer.map_start(1);
                self.writer.key(key);
                self.value(object)
                    .map_err(|failure| failure.at(|| PathSegment::Key(key.to_owned())))?;
                self.writer.map_end();
                self.depth -= 1;
                Ok(())
            }
        }
    }

    fn hinted_list(
        &mut self,
        list: &Bound<'_, PyList>,
        items: &EncodeHint,
    ) -> Result<(), EncodeFailure> {
        self.enter()?;
        self.writer.sequence_start(list.len());
        for (index, item) in list.iter().enumerate() {
            self.hinted(&item, items)
                .map_err(|failure| failure.at(|| PathSegment::Index(index)))?;
        }
        self.writer.sequence_end();
        self.depth -= 1;
        Ok(())
    }

    fn hinted_dict(
        &mut self,
        dict: &Bound<'_, PyDict>,
        values: &EncodeHint,
    ) -> Result<(), EncodeFailure> {
        self.enter()?;
        self.writer.map_start(dict.len());
        let mut entries = Vec::with_capacity(dict.len());
        for (key, value) in dict.iter() {
            entries.push((key_text(&key)?.into_owned(), value));
        }
        if self.sort_keys {
            entries.sort_by(|left, right| left.0.cmp(&right.0));
        }
        for (key, value) in entries {
            self.writer.key(&key);
            self.hinted(&value, values)
                .map_err(|failure| failure.at(|| PathSegment::Key(key.clone())))?;
        }
        self.writer.map_end();
        self.depth -= 1;
        Ok(())
    }
}
