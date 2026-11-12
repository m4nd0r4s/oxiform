//! Unions: a variant chosen by the kind of value, by a tag, or by trying each in order.

use super::numbers::{is_integer_text, is_lax_float_text, lax_bool_text, lax_integer_text};
use super::{Executor, Step, Stop, input_preview};
use crate::description::TagMode;
use crate::events::{Event, EventReader};
use crate::issues::{ContextValue, ErrorCode, PathSegment};
use crate::options::Mode;
use crate::plan::{Node, NodeId, StructId, TaggedPlan, UnionPlan};
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(crate) fn union(&mut self, union: &UnionPlan, event: Event<'input>) -> Step<S> {
        let variant = match &event {
            Event::Null => union.null,
            Event::Bool(_) => union.bool,
            Event::Number(text) => match (union.integer, union.float) {
                (Some(integer), Some(_)) if is_integer_text(text) => Some(integer),
                (_, Some(float)) => Some(float),
                (integer, None) => integer,
            },
            Event::String(_) if self.mode == Mode::Lax => return self.lax_string(union, event),
            Event::String(_) => union.string,
            Event::SequenceStart => union.sequence,
            Event::MapStart => union.map,
            Event::Key(_) | Event::MapEnd | Event::SequenceEnd | Event::End => None,
        };
        match variant {
            Some(node) => self.value(node, event),
            None => self.mismatch(&union.name, event),
        }
    }

    /// A string in lax mode. A `str` variant reads every string (`"42"` stays text in
    /// `int | str`). A variant that reads text only in one form (a `Literal`, an `Enum`, a
    /// date, base64, a `UUID`) gets the text first, and when the text does not have that form
    /// the union reads it as the integer, float or bool it spells, or as `None` when empty.
    #[cold]
    fn lax_string(&mut self, union: &UnionPlan, event: Event<'input>) -> Step<S> {
        let Event::String(text) = &event else {
            unreachable!("only strings are read here");
        };
        let lax = lax_variant(union, text);
        let (string, lax) = match (union.string, lax) {
            (None, Some(lax)) => return self.read_lax(lax, event),
            (None, None) => return self.mismatch(&union.name, event),
            (Some(string), None) => return self.value(string, event),
            (Some(_), Some(LaxVariant::Null(null))) => return self.value(null, Event::Null),
            (Some(string), Some(lax)) => (string, lax),
        };
        if matches!(self.plan.node(string), Node::String(_)) {
            return self.value(string, event);
        }
        let events = [event.clone()];
        let (outcome, issues) = self.attempt(&events, |nested, event| nested.value(string, event));
        match outcome {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) | Err(Stop::TooManyErrors) => self.read_lax(lax, event),
            Err(Stop::Unreadable) => {
                self.issues.extend(issues);
                Err(Stop::Unreadable)
            }
            Err(Stop::Sink(error)) => Err(Stop::Sink(error)),
        }
    }

    fn read_lax(&mut self, variant: LaxVariant, event: Event<'input>) -> Step<S> {
        match variant {
            LaxVariant::Null(node) => self.value(node, Event::Null),
            LaxVariant::Scalar(node) => self.value(node, event),
        }
    }

    pub(crate) fn tagged(&mut self, tagged: &TaggedPlan, event: Event<'input>) -> Step<S> {
        if event != Event::MapStart {
            return self.mismatch(&tagged.name, event);
        }
        match &tagged.mode {
            TagMode::Internal { tag } => self.internally_tagged(tagged, tag),
            TagMode::Adjacent { tag, content } => self.adjacently_tagged(tagged, tag, content),
            TagMode::External => self.externally_tagged(tagged),
        }
    }

    /// `{"type": "card", ...}`. The tag is usually the first key, and then the rest of the map
    /// is read straight as the variant; otherwise the map is recorded until the tag is found.
    fn internally_tagged(&mut self, tagged: &TaggedPlan, tag: &str) -> Step<S> {
        let first_key = match self.next_event()? {
            Event::MapEnd => {
                self.missing_tag(tagged, tag)?;
                return Ok(None);
            }
            Event::Key(key) => key,
            _ => unreachable!("readers emit a key or the end of a map inside a map"),
        };
        if first_key == tag {
            return match self.read_tag(tagged, tag)? {
                Some(id) => self.structure_entries(id),
                None => {
                    self.skip_value(Event::MapStart)?;
                    Ok(None)
                }
            };
        }
        let mut events = vec![Event::MapStart, Event::Key(first_key)];
        let value = self.next_event()?;
        self.record(value, &mut events)?;
        let mut chosen = None;
        loop {
            match self.next_event()? {
                Event::MapEnd => break,
                Event::Key(key) if key == tag && chosen.is_none() => {
                    chosen = Some(self.read_tag(tagged, tag)?);
                }
                Event::Key(key) => {
                    events.push(Event::Key(key));
                    let value = self.next_event()?;
                    self.record(value, &mut events)?;
                }
                _ => unreachable!("readers emit a key or the end of a map inside a map"),
            }
        }
        events.push(Event::MapEnd);
        match chosen {
            Some(Some(id)) => self.replay(&events, |nested, event| nested.structure(id, event)),
            Some(None) => Ok(None),
            None => {
                self.missing_tag(tagged, tag)?;
                Ok(None)
            }
        }
    }

    /// `{"type": "card", "data": {...}}`. The content is read straight when the tag comes
    /// first, and recorded otherwise.
    fn adjacently_tagged(&mut self, tagged: &TaggedPlan, tag: &str, content: &str) -> Step<S> {
        let mut chosen: Option<Option<StructId>> = None;
        let mut value: Option<Option<S::Value>> = None;
        let mut recorded: Option<Vec<Event<'input>>> = None;
        loop {
            let key = match self.next_event()? {
                Event::MapEnd => break,
                Event::Key(key) => key,
                _ => unreachable!("readers emit a key or the end of a map inside a map"),
            };
            let seen = if key == tag {
                chosen.is_some()
            } else {
                value.is_some() || recorded.is_some()
            };
            if seen || (key != tag && key != content) {
                let event = self.next_event()?;
                if seen {
                    let mark = self.issues.len();
                    let reported = self.report(
                        ErrorCode::DuplicateKey,
                        "duplicate key".into(),
                        None,
                        Vec::new(),
                    );
                    self.prefix_issues(mark, || PathSegment::Key(key.into_owned()));
                    reported?;
                }
                self.skip_value(event)?;
                continue;
            }
            if key == tag {
                chosen = Some(self.read_tag(tagged, tag)?);
                continue;
            }
            let event = self.next_event()?;
            match chosen {
                Some(Some(id)) => {
                    let mark = self.issues.len();
                    let read = self.structure(id, event);
                    self.prefix_issues(mark, || PathSegment::Key(content.to_owned()));
                    value = Some(read?);
                }
                Some(None) => {
                    self.skip_value(event)?;
                    value = Some(None);
                }
                None => {
                    let mut events = Vec::new();
                    self.record(event, &mut events)?;
                    recorded = Some(events);
                }
            }
        }
        let id = match chosen {
            Some(Some(id)) => id,
            Some(None) => return Ok(None),
            None => {
                self.missing_tag(tagged, tag)?;
                return Ok(None);
            }
        };
        if let Some(value) = value {
            return Ok(value);
        }
        let mark = self.issues.len();
        let read = match recorded {
            Some(events) => self.replay(&events, |nested, event| nested.structure(id, event)),
            None => self
                .report(
                    ErrorCode::Missing,
                    "field required".into(),
                    None,
                    Vec::new(),
                )
                .map(|()| None),
        };
        self.prefix_issues(mark, || PathSegment::Key(content.to_owned()));
        read
    }

    /// `{"card": {...}}`: a map with exactly one key, the tag.
    fn externally_tagged(&mut self, tagged: &TaggedPlan) -> Step<S> {
        let key = match self.next_event()? {
            Event::MapEnd => {
                self.report(
                    ErrorCode::Tag,
                    "expected a map with the tag as its only key".into(),
                    None,
                    allowed(tagged),
                )?;
                return Ok(None);
            }
            Event::Key(key) => key,
            _ => unreachable!("readers emit a key or the end of a map inside a map"),
        };
        let event = self.next_event()?;
        let mark = self.issues.len();
        let read = match tagged.variants.get(key.as_ref()) {
            Some(&id) => self.structure(id, event),
            None => self.unknown_tag(tagged, &key, event).map(|()| None),
        };
        self.prefix_issues(mark, || PathSegment::Key(key.into_owned()));
        let mut value = read?;
        let mut extra = false;
        loop {
            match self.next_event()? {
                Event::MapEnd => break,
                Event::Key(_) => {
                    extra = true;
                    let event = self.next_event()?;
                    self.skip_value(event)?;
                }
                _ => unreachable!("readers emit a key or the end of a map inside a map"),
            }
        }
        if extra {
            self.report(
                ErrorCode::Tag,
                "expected a map with the tag as its only key".into(),
                None,
                allowed(tagged),
            )?;
            value = None;
        }
        Ok(value)
    }

    /// Records the value, then reads it as each variant in turn until one fits.
    pub(crate) fn untagged(
        &mut self,
        variants: &[(String, NodeId)],
        event: Event<'input>,
    ) -> Step<S> {
        let preview = input_preview(&event);
        let mut events = Vec::new();
        self.record(event, &mut events)?;
        let mut failures = Vec::with_capacity(variants.len());
        for (name, node) in variants {
            let (outcome, issues) =
                self.attempt(&events, |nested, event| nested.value(*node, event));
            match outcome {
                Ok(Some(value)) => return Ok(Some(value)),
                Ok(None) | Err(Stop::TooManyErrors) => failures.push((name.clone(), issues)),
                Err(Stop::Unreadable) => {
                    self.issues.extend(issues);
                    return Err(Stop::Unreadable);
                }
                Err(Stop::Sink(error)) => return Err(Stop::Sink(error)),
            }
        }
        let names: Vec<&str> = variants.iter().map(|(name, _)| name.as_str()).collect();
        self.report(
            ErrorCode::Union,
            format!("no variant of {} fits", names.join(" | ")),
            preview,
            vec![("errors", ContextValue::Variants(failures))],
        )?;
        Ok(None)
    }

    /// Reads the value of the tag key and returns its structure, or reports why there is none.
    fn read_tag(
        &mut self,
        tagged: &TaggedPlan,
        tag: &str,
    ) -> Result<Option<StructId>, Stop<S::Error>> {
        let event = self.next_event()?;
        if let Event::String(text) = &event
            && let Some(&id) = tagged.variants.get(text.as_ref())
        {
            return Ok(Some(id));
        }
        let mark = self.issues.len();
        let reported = match &event {
            Event::String(text) => {
                let text = text.clone();
                self.unknown_tag(tagged, &text, event)
            }
            _ => self.unknown_tag(tagged, "", event),
        };
        self.prefix_issues(mark, || PathSegment::Key(tag.to_owned()));
        reported?;
        Ok(None)
    }

    fn unknown_tag(
        &mut self,
        tagged: &TaggedPlan,
        received: &str,
        event: Event<'input>,
    ) -> Result<(), Stop<S::Error>> {
        let expected = tagged
            .tags
            .iter()
            .map(|tag| format!("{tag:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let message = if matches!(event, Event::String(_)) {
            format!("unknown tag {received:?}, expected one of {expected}")
        } else {
            format!("expected the tag as a string, one of {expected}")
        };
        self.report(
            ErrorCode::Tag,
            message,
            input_preview(&event),
            allowed(tagged),
        )?;
        self.skip_value(event)
    }

    fn missing_tag(&mut self, tagged: &TaggedPlan, tag: &str) -> Result<(), Stop<S::Error>> {
        self.report(
            ErrorCode::Tag,
            format!("missing the tag key {tag:?}"),
            None,
            allowed(tagged),
        )
    }
}

/// What lax mode reads a string as when the union has no `str` variant that takes it.
enum LaxVariant {
    /// An empty string is `None`.
    Null(NodeId),
    /// The text is an integer, a float or a bool.
    Scalar(NodeId),
}

/// The variant that lax mode reads `text` as: `None` for an empty string, then the first of
/// integer, float and bool that the text is valid for.
fn lax_variant(union: &UnionPlan, text: &str) -> Option<LaxVariant> {
    if let (true, true, Some(null)) = (text.is_empty(), union.empty_is_none, union.null) {
        return Some(LaxVariant::Null(null));
    }
    if lax_integer_text(text).is_some()
        && let Some(integer) = union.integer
    {
        return Some(LaxVariant::Scalar(integer));
    }
    if is_lax_float_text(text)
        && let Some(float) = union.float
    {
        return Some(LaxVariant::Scalar(float));
    }
    if lax_bool_text(text).is_some()
        && let Some(boolean) = union.bool
    {
        return Some(LaxVariant::Scalar(boolean));
    }
    None
}

fn allowed(tagged: &TaggedPlan) -> Vec<(&'static str, ContextValue)> {
    vec![("allowed", ContextValue::Texts(tagged.tags.clone()))]
}
