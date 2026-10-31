//! Lists, dicts and untyped values.

use std::collections::HashSet;

use super::numbers::is_integer_text;
use super::{Executor, Step};
use crate::events::{Event, EventReader};
use crate::issues::{ErrorCode, PathSegment};
use crate::plan::{LengthRange, NodeId};
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, R, S> {
    /// A value of any type, built from what the input holds.
    pub(crate) fn any(&mut self, event: Event<'input>) -> Step<S> {
        match event {
            Event::Null => self.emit(Sink::none),
            Event::Bool(value) => self.emit(|sink| sink.bool(value)),
            Event::Number(text) if is_integer_text(text) => {
                let value = self.read_integer(text)?;
                self.emit_integer(value)
            }
            Event::Number(text) => match self.read_float(text)? {
                Some(value) => self.emit(|sink| sink.float(value)),
                None => Ok(None),
            },
            Event::String(text) => {
                self.check_string_size(&text)?;
                self.emit(|sink| sink.string(&text))
            }
            Event::SequenceStart => self.list(None, LengthRange::default(), event),
            Event::MapStart => self.dict(None, LengthRange::default(), event),
            Event::Key(_) | Event::MapEnd | Event::SequenceEnd | Event::End => {
                unreachable!("readers only start values with value events")
            }
        }
    }

    /// A list whose items are read as `items`, or as any type when `None`.
    pub(crate) fn list(
        &mut self,
        items: Option<NodeId>,
        length: LengthRange,
        event: Event<'input>,
    ) -> Step<S> {
        if event != Event::SequenceStart {
            return self.mismatch("list", event);
        }
        let mut values = Vec::new();
        let mut valid = true;
        let mut index = 0;
        loop {
            let event = self.next_event()?;
            if event == Event::SequenceEnd {
                break;
            }
            if index >= self.limits.max_items {
                return Err(self.limit_exceeded("max_items", self.limits.max_items, "the list"));
            }
            self.path.push(PathSegment::Index(index));
            let item = self.element(items, event);
            self.path.pop();
            match item? {
                Some(item) if valid => values.push(item),
                Some(_) => {}
                None => valid = false,
            }
            index += 1;
        }
        if !length.is_unbounded() {
            valid &= self.check_length(length, index, || None)?;
        }
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.list(values))
    }

    /// A dict with string keys whose values are read as `values`, or as any type when `None`.
    pub(crate) fn dict(
        &mut self,
        values: Option<NodeId>,
        length: LengthRange,
        event: Event<'input>,
    ) -> Step<S> {
        if event != Event::MapStart {
            return self.mismatch("dict", event);
        }
        let mut entries = Vec::new();
        let mut keys = HashSet::new();
        let mut valid = true;
        loop {
            let key = match self.next_event()? {
                Event::MapEnd => break,
                Event::Key(key) => key,
                _ => unreachable!("readers emit a key or the end of a map inside a map"),
            };
            if keys.len() >= self.limits.max_items {
                return Err(self.limit_exceeded("max_items", self.limits.max_items, "the dict"));
            }
            self.check_string_size(&key)?;
            let value_event = self.next_event()?;
            self.path.push(PathSegment::Key(key.to_string()));
            if !keys.insert(key.clone()) {
                let duplicate = self.report(
                    ErrorCode::DuplicateKey,
                    "duplicate key".into(),
                    Some(key.to_string()),
                    Vec::new(),
                );
                self.path.pop();
                duplicate?;
                self.skip_value(value_event)?;
                valid = false;
                continue;
            }
            let value = self.element(values, value_event);
            self.path.pop();
            match value? {
                Some(value) if valid => {
                    let key = self
                        .emit(|sink| sink.string(&key))?
                        .expect("emit returns a value");
                    entries.push((key, value));
                }
                Some(_) => {}
                None => valid = false,
            }
        }
        if !length.is_unbounded() {
            valid &= self.check_length(length, keys.len(), || None)?;
        }
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.dict(entries))
    }

    fn element(&mut self, node: Option<NodeId>, event: Event<'input>) -> Step<S> {
        match node {
            Some(node) => self.value(node, event),
            None => self.any(event),
        }
    }
}
