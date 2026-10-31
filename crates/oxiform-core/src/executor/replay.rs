//! Values read ahead and read again: the variants of an untagged union, and tagged maps whose
//! tag is not the first key.

use super::{Executor, Step, Stop};
use crate::events::{Event, EventReader, ReaderError};
use crate::issues::Issue;
use crate::sink::Sink;

/// A reader over events recorded earlier.
pub(crate) struct ReplayReader<'events, 'input> {
    events: &'events [Event<'input>],
    position: usize,
}

impl<'input> EventReader<'input> for ReplayReader<'_, 'input> {
    fn next_event(&mut self) -> Result<Event<'input>, ReaderError> {
        let event = self.events.get(self.position).cloned();
        self.position += 1;
        Ok(event.unwrap_or(Event::End))
    }

    fn offset(&self) -> usize {
        0
    }
}

/// The outcome of reading recorded events, and the issues found on the way.
pub(crate) type Attempt<S> = (Step<S>, Vec<Issue>);

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    /// Appends the value that starts with `event` to `events`, reading the rest of it.
    pub(crate) fn record(
        &mut self,
        event: Event<'input>,
        events: &mut Vec<Event<'input>>,
    ) -> Result<(), Stop<S::Error>> {
        let mut depth: usize = match event {
            Event::MapStart | Event::SequenceStart => 1,
            _ => 0,
        };
        events.push(event);
        while depth > 0 {
            let event = self.next_event()?;
            match event {
                Event::MapStart | Event::SequenceStart => depth += 1,
                Event::MapEnd | Event::SequenceEnd => depth -= 1,
                _ => {}
            }
            events.push(event);
        }
        Ok(())
    }

    /// Reads `events` with `read`, keeping its issues apart from those of this run.
    pub(crate) fn attempt<'events>(
        &mut self,
        events: &'events [Event<'input>],
        read: impl FnOnce(
            &mut Executor<'_, 'input, ReplayReader<'events, 'input>, S>,
            Event<'input>,
        ) -> Step<S>,
    ) -> Attempt<S> {
        let mut reader = ReplayReader {
            events,
            position: 1,
        };
        let mut nested = Executor {
            marker: std::marker::PhantomData,
            plan: self.plan,
            reader: &mut reader,
            sink: &mut *self.sink,
            limits: self.limits,
            issues: Vec::new(),
        };
        let outcome = read(&mut nested, events[0].clone());
        (outcome, nested.issues)
    }

    /// Reads `events` with `read`, as if they came from the input now.
    pub(crate) fn replay<'events>(
        &mut self,
        events: &'events [Event<'input>],
        read: impl FnOnce(
            &mut Executor<'_, 'input, ReplayReader<'events, 'input>, S>,
            Event<'input>,
        ) -> Step<S>,
    ) -> Step<S> {
        let (outcome, issues) = self.attempt(events, read);
        self.absorb(issues)?;
        outcome
    }

    /// Adds issues found while reading recorded events.
    pub(crate) fn absorb(&mut self, issues: Vec<Issue>) -> Result<(), Stop<S::Error>> {
        self.issues.extend(issues);
        if self.issues.len() >= self.limits.max_errors.max(1) {
            return Err(Stop::TooManyErrors);
        }
        Ok(())
    }
}
