//! A validating pull reader that turns JSON text into events.

use oxiform_core::events::{Event, EventReader, ReaderError, ReaderErrorKind};

use super::location::line_and_column;
use super::number::scan_number;
use super::string::read_string;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Map,
    Sequence,
}

/// What the reader expects next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// A value: at the top level, after a colon, or after a comma in a sequence.
    Value,
    /// Just after `[`: a value or `]`.
    FirstInSequence,
    /// Just after `{`: a key or `}`.
    FirstInMap,
    /// After a comma in a map: a key.
    Key,
    /// After a value: a comma or the end of the enclosing container.
    AfterValue,
    /// The top-level value is complete; only whitespace may follow.
    Done,
    /// `Event::End` was returned.
    Finished,
}

/// Reads one JSON document.
pub struct JsonReader<'input> {
    input: &'input str,
    position: usize,
    event_offset: usize,
    containers: Vec<Container>,
    state: State,
    max_depth: usize,
}

impl<'input> JsonReader<'input> {
    /// A reader that refuses containers nested deeper than `max_depth`.
    pub fn new(input: &'input str, max_depth: usize) -> Self {
        Self {
            input,
            position: 0,
            event_offset: 0,
            containers: Vec::new(),
            state: State::Value,
            max_depth,
        }
    }

    fn bytes(&self) -> &'input [u8] {
        self.input.as_bytes()
    }

    fn skip_whitespace(&mut self) {
        let bytes = self.bytes();
        while matches!(bytes.get(self.position), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes().get(self.position).copied()
    }

    fn error(
        &self,
        offset: usize,
        kind: ReaderErrorKind,
        message: impl Into<String>,
    ) -> ReaderError {
        let (line, column) = line_and_column(self.input, offset);
        ReaderError {
            kind,
            message: message.into(),
            offset,
            line,
            column,
        }
    }

    fn syntax_error(&self, offset: usize, message: impl Into<String>) -> ReaderError {
        self.error(offset, ReaderErrorKind::Syntax, message)
    }

    fn unexpected(&self, expected: &str) -> ReaderError {
        match self.peek() {
            None => self.syntax_error(
                self.position,
                format!("unexpected end of input, expected {expected}"),
            ),
            Some(_) => {
                let found = self.input[self.position..]
                    .chars()
                    .next()
                    .unwrap_or_default();
                self.syntax_error(
                    self.position,
                    format!("expected {expected}, found {found:?}"),
                )
            }
        }
    }

    fn open(&mut self, container: Container) -> Result<(), ReaderError> {
        if self.containers.len() >= self.max_depth {
            let limit = self.max_depth;
            return Err(self.error(
                self.position,
                ReaderErrorKind::DepthLimit { limit },
                format!("nesting deeper than the limit of {limit} levels"),
            ));
        }
        self.containers.push(container);
        self.position += 1;
        Ok(())
    }

    fn close(&mut self) {
        self.containers.pop();
        self.position += 1;
        self.state = State::AfterValue;
    }

    fn read_value(&mut self) -> Result<Event<'input>, ReaderError> {
        self.skip_whitespace();
        self.event_offset = self.position;
        let event = match self.peek() {
            Some(b'{') => {
                self.open(Container::Map)?;
                self.state = State::FirstInMap;
                return Ok(Event::MapStart);
            }
            Some(b'[') => {
                self.open(Container::Sequence)?;
                self.state = State::FirstInSequence;
                return Ok(Event::SequenceStart);
            }
            Some(b'"') => {
                let (text, end) = read_string(self.input, self.position)
                    .map_err(|(offset, message)| self.syntax_error(offset, message))?;
                self.position = end;
                Event::String(text)
            }
            Some(b'-' | b'0'..=b'9') => {
                let end = scan_number(self.bytes(), self.position)
                    .map_err(|(offset, message)| self.syntax_error(offset, message))?;
                let text = &self.input[self.position..end];
                self.position = end;
                Event::Number(text)
            }
            Some(b't') => self.read_literal("true", Event::Bool(true))?,
            Some(b'f') => self.read_literal("false", Event::Bool(false))?,
            Some(b'n') => self.read_literal("null", Event::Null)?,
            _ => return Err(self.unexpected("a value")),
        };
        self.state = State::AfterValue;
        Ok(event)
    }

    fn read_literal(
        &mut self,
        literal: &str,
        event: Event<'input>,
    ) -> Result<Event<'input>, ReaderError> {
        if self.bytes()[self.position..].starts_with(literal.as_bytes()) {
            self.position += literal.len();
            Ok(event)
        } else {
            Err(self.unexpected("a value"))
        }
    }

    fn read_key(&mut self) -> Result<Event<'input>, ReaderError> {
        self.event_offset = self.position;
        let (key, end) = read_string(self.input, self.position)
            .map_err(|(offset, message)| self.syntax_error(offset, message))?;
        self.position = end;
        self.skip_whitespace();
        if self.peek() != Some(b':') {
            return Err(self.unexpected("':' after the key"));
        }
        self.position += 1;
        self.state = State::Value;
        Ok(Event::Key(key))
    }

    fn after_value(&mut self) -> Result<Option<Event<'input>>, ReaderError> {
        self.skip_whitespace();
        self.event_offset = self.position;
        match (self.containers.last(), self.peek()) {
            (None, _) => {
                self.state = State::Done;
                Ok(None)
            }
            (Some(Container::Map), Some(b',')) => {
                self.position += 1;
                self.state = State::Key;
                Ok(None)
            }
            (Some(Container::Map), Some(b'}')) => {
                self.close();
                Ok(Some(Event::MapEnd))
            }
            (Some(Container::Sequence), Some(b',')) => {
                self.position += 1;
                self.state = State::Value;
                Ok(None)
            }
            (Some(Container::Sequence), Some(b']')) => {
                self.close();
                Ok(Some(Event::SequenceEnd))
            }
            (Some(Container::Map), _) => Err(self.unexpected("',' or '}'")),
            (Some(Container::Sequence), _) => Err(self.unexpected("',' or ']'")),
        }
    }
}

impl<'input> EventReader<'input> for JsonReader<'input> {
    fn next_event(&mut self) -> Result<Event<'input>, ReaderError> {
        loop {
            match self.state {
                State::Value => return self.read_value(),
                State::FirstInSequence => {
                    self.skip_whitespace();
                    if self.peek() == Some(b']') {
                        self.event_offset = self.position;
                        self.close();
                        return Ok(Event::SequenceEnd);
                    }
                    return self.read_value();
                }
                State::FirstInMap => {
                    self.skip_whitespace();
                    match self.peek() {
                        Some(b'}') => {
                            self.event_offset = self.position;
                            self.close();
                            return Ok(Event::MapEnd);
                        }
                        Some(b'"') => return self.read_key(),
                        _ => return Err(self.unexpected("a string key or '}'")),
                    }
                }
                State::Key => {
                    self.skip_whitespace();
                    if self.peek() != Some(b'"') {
                        return Err(self.unexpected("a string key"));
                    }
                    return self.read_key();
                }
                State::AfterValue => {
                    if let Some(event) = self.after_value()? {
                        return Ok(event);
                    }
                }
                State::Done => {
                    self.skip_whitespace();
                    if self.peek().is_some() {
                        return Err(self.unexpected("the end of the document"));
                    }
                    self.event_offset = self.position;
                    self.state = State::Finished;
                    return Ok(Event::End);
                }
                State::Finished => return Ok(Event::End),
            }
        }
    }

    fn offset(&self) -> usize {
        self.event_offset
    }
}
