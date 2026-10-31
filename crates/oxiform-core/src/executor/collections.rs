//! Lists, dicts and untyped values.

use super::numbers::is_integer_text;
use super::{Executor, PathStep, Step, Stop};
use crate::events::{Event, EventReader};
use crate::issues::ErrorCode;
use crate::plan::{LengthRange, NodeId};
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
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
            self.path.push(PathStep::Index(index));
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
        let mut map = self.sink.map_new().map_err(Stop::Sink)?;
        let mut valid = true;
        let mut count = 0;
        loop {
            let key = match self.next_event()? {
                Event::MapEnd => break,
                Event::Key(key) => key,
                _ => unreachable!("readers emit a key or the end of a map inside a map"),
            };
            if count >= self.limits.max_items {
                return Err(self.limit_exceeded("max_items", self.limits.max_items, "the dict"));
            }
            count += 1;
            self.check_string_size(&key)?;
            self.path.push(PathStep::Key(key));
            let entry = self.entry(values, &mut map, valid);
            self.path.pop();
            valid &= entry?;
        }
        if !length.is_unbounded() {
            valid &= self.check_length(length, count, || None)?;
        }
        if !valid {
            return Ok(None);
        }
        self.sink.map_finish(map).map(Some).map_err(Stop::Sink)
    }

    /// Reads the value of the key at the top of the path into `map` (only while the dict is
    /// still valid). Returns whether the entry is valid; a repeated key is an issue.
    fn entry(
        &mut self,
        values: Option<NodeId>,
        map: &mut S::Map,
        insert: bool,
    ) -> Result<bool, Stop<S::Error>> {
        let value_event = self.next_event()?;
        let Some(value) = self.element(values, value_event)? else {
            return Ok(false);
        };
        if !insert {
            return Ok(true);
        }
        let Some(PathStep::Key(key)) = self.path.last() else {
            unreachable!("the key is on the path")
        };
        let key = key.clone();
        if self.sink.map_insert(map, &key, value).map_err(Stop::Sink)? {
            return Ok(true);
        }
        self.report(
            ErrorCode::DuplicateKey,
            "duplicate key".into(),
            Some(key.into_owned()),
            Vec::new(),
        )?;
        Ok(false)
    }

    fn element(&mut self, node: Option<NodeId>, event: Event<'input>) -> Step<S> {
        match node {
            Some(node) => self.value(node, event),
            None => self.any(event),
        }
    }
}
