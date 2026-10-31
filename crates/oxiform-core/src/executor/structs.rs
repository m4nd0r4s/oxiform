//! Structures: named fields read from a map.

use super::{Executor, Step};
use crate::description::UnknownFields;
use crate::events::{Event, EventReader};
use crate::issues::{ErrorCode, PathSegment};
use crate::plan::StructId;
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, R, S> {
    pub(crate) fn structure(&mut self, id: StructId, event: Event<'input>) -> Step<S> {
        let plan = self.plan;
        let structure = plan.struct_plan(id);
        if event != Event::MapStart {
            return self.mismatch(&structure.name, event);
        }
        let mut slots: Vec<Option<S::Value>> = structure.fields.iter().map(|_| None).collect();
        let mut seen = vec![false; structure.fields.len()];
        let mut valid = true;
        let mut keys = 0;
        loop {
            let key = match self.next_event()? {
                Event::MapEnd => break,
                Event::Key(key) => key,
                _ => unreachable!("readers emit a key or the end of a map inside a map"),
            };
            keys += 1;
            if keys > self.limits.max_items {
                return Err(self.limit_exceeded("max_items", self.limits.max_items, "the map"));
            }
            self.check_string_size(&key)?;
            let value_event = self.next_event()?;
            let Some(index) = structure.field_index(&key) else {
                if structure.unknown_fields == UnknownFields::Forbid {
                    self.path.push(PathSegment::Key(key.to_string()));
                    let reported = self.report(
                        ErrorCode::UnknownField,
                        "unknown field".into(),
                        None,
                        Vec::new(),
                    );
                    self.path.pop();
                    reported?;
                    valid = false;
                }
                self.skip_value(value_event)?;
                continue;
            };
            self.path.push(PathSegment::Key(key.to_string()));
            if seen[index] {
                let reported = self.report(
                    ErrorCode::DuplicateKey,
                    "duplicate key".into(),
                    Some(key.to_string()),
                    Vec::new(),
                );
                self.path.pop();
                reported?;
                self.skip_value(value_event)?;
                valid = false;
                continue;
            }
            seen[index] = true;
            let value = self.value(structure.fields[index].value, value_event);
            self.path.pop();
            match value? {
                Some(value) => slots[index] = Some(value),
                None => valid = false,
            }
        }
        for (field, was_seen) in structure.fields.iter().zip(&seen) {
            if !was_seen && !field.has_default {
                self.path.push(PathSegment::Key(field.wire_name.clone()));
                let reported = self.report(
                    ErrorCode::Missing,
                    "field required".into(),
                    None,
                    Vec::new(),
                );
                self.path.pop();
                reported?;
                valid = false;
            }
        }
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.instance(id, slots))
    }
}
