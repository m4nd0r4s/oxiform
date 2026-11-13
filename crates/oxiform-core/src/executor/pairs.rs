//! Sources of pairs (query strings, forms, headers, cookies) read as a structure.
//!
//! A reader of pairs emits one map in which each key appears once and its values come as a
//! sequence, whatever their number. This is the only place that knows what that means: the
//! structure at the root decides, per field, whether the sequence is the value (a `list`, a
//! `tuple`, a `set`) or whether the field takes the one value in it. It is a function of its own,
//! apart from `structure_entries`, so that reading JSON does not pay for any of it.

use smallvec::{SmallVec, smallvec};

use super::{Executor, Step, Stop};
use crate::description::UnknownFields;
use crate::events::{Event, EventReader};
use crate::issues::{ErrorCode, PathSegment};
use crate::plan::{Node, NodeId, StructPlan};
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(super) fn run_pairs(&mut self, fold_case: bool) -> Step<S> {
        let plan = self.plan;
        let Node::Struct(id) = plan.node(plan.root()) else {
            self.report(
                ErrorCode::Type,
                "pairs can only be read as a model: declare the parameters as a class with one \
                 field per name"
                    .into(),
                None,
                Vec::new(),
            )?;
            return Ok(None);
        };
        let structure = plan.struct_plan(*id);
        let start = self.next_event()?;
        debug_assert_eq!(start, Event::MapStart);
        let mut values: SmallVec<[Option<S::Value>; 16]> =
            structure.fields.iter().map(|_| None).collect();
        let mut seen: SmallVec<[bool; 16]> = smallvec![false; structure.fields.len()];
        let mut valid = true;
        let mut keys = 0;
        loop {
            let key = match self.next_event()? {
                Event::MapEnd => break,
                Event::Key(key) => key,
                _ => unreachable!("readers of pairs emit a key or the end of the map"),
            };
            keys += 1;
            if keys > self.limits.max_items {
                return Err(self.limit_exceeded("max_items", self.limits.max_items, "the pairs"));
            }
            let index = if fold_case {
                structure.field_index_folded(&key)
            } else {
                structure.field_index(&key)
            };
            // Issues are recorded without a path, and the key goes in front on the way out.
            let name = match index {
                Some(index) if fold_case => structure.fields[index].wire_name.clone(),
                _ => key.into_owned(),
            };
            let mark = self.issues.len();
            let field = self.pair_field(structure, index, &mut values, &mut seen, &name);
            self.prefix_issues(mark, || PathSegment::Key(name));
            valid &= field?;
        }
        valid &= self.pairs_missing(structure, &seen, &mut values)?;
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.instance(*id, &mut values))
    }

    /// Reads the values of one key, which start with the sequence the reader emits. Returns
    /// whether they are valid.
    fn pair_field(
        &mut self,
        structure: &StructPlan,
        index: Option<usize>,
        values: &mut [Option<S::Value>],
        seen: &mut [bool],
        name: &str,
    ) -> Result<bool, Stop<S::Error>> {
        let sequence = self.next_event()?;
        debug_assert_eq!(sequence, Event::SequenceStart);
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
            self.skip_value(sequence)?;
            return Ok(!forbidden);
        };
        if seen[index] {
            // Two names of one field (an alias and the field's own name).
            self.skip_value(sequence)?;
            return self.duplicate_pair(name);
        }
        seen[index] = true;
        let node = structure.fields[index].value;
        let value = if self.wants_sequence(node) {
            self.value(node, sequence)?
        } else {
            let first = self.next_event()?;
            let second = self.next_event()?;
            if second != Event::SequenceEnd {
                self.skip_rest_of_sequence(second)?;
                return self.duplicate_pair(name);
            }
            self.value(node, first)?
        };
        match value {
            Some(value) => {
                values[index] = Some(value);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// A key given more than once to a field that holds one value.
    fn duplicate_pair(&mut self, name: &str) -> Result<bool, Stop<S::Error>> {
        self.report(
            ErrorCode::DuplicateKey,
            "duplicate key: this name was given more than once, and the field takes one value; \
             declare it as a list to take several"
                .into(),
            Some(name.to_owned()),
            Vec::new(),
        )?;
        Ok(false)
    }

    /// Consumes the rest of a sequence, given the event that was read last and is not its end.
    fn skip_rest_of_sequence(&mut self, mut event: Event<'input>) -> Result<(), Stop<S::Error>> {
        while event != Event::SequenceEnd {
            event = self.next_event()?;
        }
        Ok(())
    }

    /// Whether a field reads the sequence of its values (a list, a tuple, a set), possibly
    /// optional, and not one value of them.
    fn wants_sequence(&self, mut node: NodeId) -> bool {
        loop {
            match self.plan.node(node) {
                Node::Optional { inner, .. } | Node::Secret(inner) => node = *inner,
                Node::List { .. } | Node::Tuple(_) => return true,
                _ => return false,
            }
        }
    }

    /// Reports the required fields that no pair gave. Returns whether none is missing.
    ///
    /// A field that is optional (`T | None`) is never required here: with no pair and no default
    /// it is `None`, which is what a query string or a header that is absent means.
    fn pairs_missing(
        &mut self,
        structure: &StructPlan,
        seen: &[bool],
        values: &mut [Option<S::Value>],
    ) -> Result<bool, Stop<S::Error>> {
        let mut complete = true;
        for (index, (field, was_seen)) in structure.fields.iter().zip(seen).enumerate() {
            if *was_seen || field.has_default {
                continue;
            }
            if matches!(self.plan.node(field.value), Node::Optional { .. }) {
                values[index] = self.emit(Sink::none)?;
            } else {
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
