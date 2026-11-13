//! The events of a set of pairs.

use std::borrow::Cow;
use std::vec::IntoIter;

use oxiform_core::events::{Event, EventReader, ReaderError};

pub(super) type Text<'input> = Result<Cow<'input, str>, ReaderError>;

/// One name and the values it was given. A fault found while decoding is kept where it was
/// found, so the executor meets it in reading order, with the name in front of its path.
pub(super) struct Group<'input> {
    pub(super) name: Text<'input>,
    pub(super) values: Vec<Text<'input>>,
}

#[derive(Debug, Clone, Copy)]
enum Next {
    MapStart,
    Name,
    SequenceStart,
    Value,
    MapEnd,
    End,
}

/// Emits one map: each name once, in order of first appearance, and its values as a sequence
/// (also when it has one). What to make of that sequence is for the field to decide.
pub struct PairsReader<'input> {
    groups: IntoIter<Group<'input>>,
    values: IntoIter<Text<'input>>,
    next: Next,
}

impl<'input> PairsReader<'input> {
    pub(super) fn new(groups: Vec<Group<'input>>) -> Self {
        Self {
            groups: groups.into_iter(),
            values: Vec::new().into_iter(),
            next: Next::MapStart,
        }
    }
}

impl<'input> EventReader<'input> for PairsReader<'input> {
    fn next_event(&mut self) -> Result<Event<'input>, ReaderError> {
        loop {
            match self.next {
                Next::MapStart => {
                    self.next = Next::Name;
                    return Ok(Event::MapStart);
                }
                Next::Name => match self.groups.next() {
                    None => self.next = Next::MapEnd,
                    Some(group) => {
                        let name = group.name?;
                        self.values = group.values.into_iter();
                        self.next = Next::SequenceStart;
                        return Ok(Event::Key(name));
                    }
                },
                Next::SequenceStart => {
                    self.next = Next::Value;
                    return Ok(Event::SequenceStart);
                }
                Next::Value => {
                    return match self.values.next() {
                        None => {
                            self.next = Next::Name;
                            Ok(Event::SequenceEnd)
                        }
                        Some(value) => value.map(Event::String),
                    };
                }
                Next::MapEnd => {
                    self.next = Next::End;
                    return Ok(Event::MapEnd);
                }
                Next::End => return Ok(Event::End),
            }
        }
    }

    /// Pairs have no position in a document: always 0.
    fn offset(&self) -> usize {
        0
    }
}
