//! Structures: named fields read from a map.

use std::borrow::Cow;

use smallvec::{SmallVec, smallvec};

use super::{Executor, PathStep, Step, Stop};
use crate::description::UnknownFields;
use crate::events::{Event, EventReader};
use crate::issues::ErrorCode;
use crate::plan::{StructId, StructPlan};
use crate::sink::Sink;

/// The fields read so far.
struct Slots<V> {
    values: SmallVec<[Option<V>; 16]>,
    seen: SmallVec<[bool; 16]>,
    /// The field expected next, assuming keys come in declaration order.
    expected: usize,
}

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(crate) fn structure(&mut self, id: StructId, event: Event<'input>) -> Step<S> {
        let plan = self.plan;
        let structure = plan.struct_plan(id);
        if event != Event::MapStart {
            return self.mismatch(&structure.name, event);
        }
        let mut slots = Slots {
            values: structure.fields.iter().map(|_| None).collect(),
            seen: smallvec![false; structure.fields.len()],
            expected: 0,
        };
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
            let index = structure.find_field(&key, slots.expected);
            self.path.push(PathStep::Key(key));
            let field = self.field(structure, index, &mut slots);
            self.path.pop();
            valid &= field?;
        }
        valid &= self.check_missing(structure, &slots.seen)?;
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.instance(id, &mut slots.values))
    }

    /// Reads the value of field `index` (`None`: an unknown key), with the path already at
    /// the key. Returns whether it is valid.
    fn field(
        &mut self,
        structure: &StructPlan,
        index: Option<usize>,
        slots: &mut Slots<S::Value>,
    ) -> Result<bool, Stop<S::Error>> {
        let value_event = self.next_event()?;
        let Some(index) = index else {
            let forbidden = structure.unknown_fields == UnknownFields::Forbid;
            if forbidden {
                self.report(
                    ErrorCode::UnknownField,
                    "unknown field".into(),
                    None,
                    Vec::new(),
                )?;
            }
            self.skip_value(value_event)?;
            return Ok(!forbidden);
        };
        if slots.seen[index] {
            let key = structure.fields[index].wire_name.clone();
            self.report(
                ErrorCode::DuplicateKey,
                "duplicate key".into(),
                Some(key),
                Vec::new(),
            )?;
            self.skip_value(value_event)?;
            return Ok(false);
        }
        slots.seen[index] = true;
        slots.expected = index + 1;
        match self.value(structure.fields[index].value, value_event)? {
            Some(value) => {
                slots.values[index] = Some(value);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Reports required fields that never appeared. Returns whether none is missing.
    fn check_missing(
        &mut self,
        structure: &StructPlan,
        seen: &[bool],
    ) -> Result<bool, Stop<S::Error>> {
        let mut complete = true;
        for (field, was_seen) in structure.fields.iter().zip(seen) {
            if !was_seen && !field.has_default {
                self.path
                    .push(PathStep::Key(Cow::Owned(field.wire_name.clone())));
                let reported = self.report(
                    ErrorCode::Missing,
                    "field required".into(),
                    None,
                    Vec::new(),
                );
                self.path.pop();
                reported?;
                complete = false;
            }
        }
        Ok(complete)
    }
}
