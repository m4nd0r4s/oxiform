//! Structures: named fields read from a map.

use smallvec::{SmallVec, smallvec};

use super::{Executor, Step, Stop};
use crate::description::UnknownFields;
use crate::events::{Event, EventReader};
use crate::issues::{ContextValue, ErrorCode, Issue, PathSegment};
use crate::plan::{StructId, StructPlan};
use crate::sink::{Rejection, Sink, Verdict};

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
        self.structure_entries(id)
    }

    /// Reads the entries of a structure whose map start was already read, up to its end.
    pub(crate) fn structure_entries(&mut self, id: StructId) -> Step<S> {
        let plan = self.plan;
        let structure = plan.struct_plan(id);
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
            if let Some((tag_field, tag)) = &structure.tag
                && *key == **tag_field
            {
                let mark = self.issues.len();
                let value_event = self.next_event()?;
                let matched = self.check_tag(tag, value_event);
                self.prefix_issues(mark, || PathSegment::Key(key.into_owned()));
                valid &= matched?;
                continue;
            }
            let index = structure.find_field(&key, slots.expected);
            let mark = self.issues.len();
            let field = self.field(structure, index, &mut slots);
            self.prefix_issues(mark, || PathSegment::Key(key.into_owned()));
            valid &= field?;
        }
        valid &= self.check_missing(structure, &slots.seen)?;
        self.finish_structure(id, structure, valid, &mut slots.values, &slots.seen)
    }

    /// Builds the instance of a structure whose map was read, running its validators. `valid`
    /// is whether every field was read without a problem; `seen` tells which fields the input
    /// gave, and only those are validated: a default was checked when the plan was compiled.
    pub(super) fn finish_structure(
        &mut self,
        id: StructId,
        structure: &StructPlan,
        mut valid: bool,
        values: &mut [Option<S::Value>],
        seen: &[bool],
    ) -> Step<S> {
        if structure.field_validators {
            valid &= self.check_fields(id, structure, values, seen)?;
        }
        if !valid {
            return Ok(None);
        }
        let instance = self.emit(|sink| sink.instance(id, values))?;
        match instance {
            Some(instance) if structure.model_validators => self.check_instance(id, instance),
            other => Ok(other),
        }
    }

    /// Runs the validators of the fields the input gave, in field order, each on a field that
    /// is valid so far. Returns whether every one accepted its value.
    #[cold]
    fn check_fields(
        &mut self,
        id: StructId,
        structure: &StructPlan,
        values: &mut [Option<S::Value>],
        seen: &[bool],
    ) -> Result<bool, Stop<S::Error>> {
        let mut valid = true;
        for (index, field) in structure.fields.iter().enumerate() {
            if !field.validated || !seen[index] {
                continue;
            }
            let Some(value) = values[index].take() else {
                continue;
            };
            let mark = self.issues.len();
            let checked = match self.sink.check_field(id, index, value) {
                Ok(Verdict::Accept(value)) => {
                    values[index] = Some(value);
                    Ok(())
                }
                Ok(Verdict::Reject(rejections)) => {
                    valid = false;
                    self.record_rejections(rejections)
                }
                Err(error) => Err(Stop::Sink(error)),
            };
            self.prefix_issues(mark, || PathSegment::Key(field.wire_name.clone()));
            checked?;
        }
        Ok(valid)
    }

    /// Runs the validators of a structure on its instance. A rejection is at the structure: the
    /// containers around it add their steps on the way out.
    #[cold]
    fn check_instance(&mut self, id: StructId, instance: S::Value) -> Step<S> {
        match self.sink.check_model(id, instance) {
            Ok(Verdict::Accept(instance)) => Ok(Some(instance)),
            Ok(Verdict::Reject(rejections)) => {
                self.record_rejections(rejections)?;
                Ok(None)
            }
            Err(error) => Err(Stop::Sink(error)),
        }
    }

    /// Records what a validator rejected as issues of code `invalid`, at the paths it gave.
    fn record_rejections(&mut self, rejections: Vec<Rejection>) -> Result<(), Stop<S::Error>> {
        for rejection in rejections {
            self.issues.push(Issue {
                code: ErrorCode::Invalid,
                user_code: rejection.code,
                path: rejection.path,
                message: rejection.message,
                input: None,
                context: Vec::new(),
            });
            if self.issues.len() >= self.max_errors {
                return Err(Stop::TooManyErrors);
            }
        }
        Ok(())
    }

    /// Reads the value of field `index` (`None`: an unknown key),  Returns whether it is valid.
    fn field(
        &mut self,
        structure: &StructPlan,
        index: Option<usize>,
        slots: &mut Slots<S::Value>,
    ) -> Result<bool, Stop<S::Error>> {
        let value_event = self.next_event()?;
        let Some(index) = index else {
            let policy = self.unknown.unwrap_or(structure.unknown_fields);
            let forbidden = policy == UnknownFields::Forbid;
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

    /// Checks that the tag key of a structure read on its own holds the structure's tag.
    fn check_tag(&mut self, tag: &str, event: Event<'input>) -> Result<bool, Stop<S::Error>> {
        if matches!(&event, Event::String(text) if text == tag) {
            return Ok(true);
        }
        self.report(
            ErrorCode::Tag,
            format!("expected the tag {tag:?}"),
            super::input_preview(&event),
            vec![("allowed", ContextValue::Texts(vec![tag.to_owned()]))],
        )?;
        self.skip_value(event)?;
        Ok(false)
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
                let mark = self.issues.len();
                let reported = self.report(
                    ErrorCode::Missing,
                    "field required".into(),
                    None,
                    Vec::new(),
                );
                self.prefix_issues(mark, || PathSegment::Key(field.wire_name.clone()));
                reported?;
                complete = false;
            }
        }
        Ok(complete)
    }
}
