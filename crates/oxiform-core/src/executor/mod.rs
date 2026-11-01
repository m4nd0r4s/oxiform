//! Runs a plan against reader events in a single pass, building the output through a sink and
//! collecting every issue on the way.

mod choices;
mod collections;
mod conversions;
mod numbers;
mod replay;
mod scalars;
mod structs;
mod temporal;
mod unions;

use crate::events::{Event, EventReader, ReaderError, ReaderErrorKind};
use crate::issues::{ContextValue, ErrorCode, Issue, PathSegment, Report, ReportKind};
use crate::limits::Limits;
use crate::plan::{Node, NodeId, Plan};
use crate::sink::Sink;

/// Characters of the received value kept in an issue.
const INPUT_PREVIEW_CHARACTERS: usize = 80;

/// Why a run produced no value.
#[derive(Debug)]
pub enum Failure<SinkError> {
    /// The input is malformed or does not match the type.
    Invalid(Report),
    /// The sink failed, for example a Python call raised.
    Sink(SinkError),
}

/// Reads one document from `reader` as the type of `plan`.
pub fn execute<'input, R, S>(
    plan: &Plan,
    reader: &mut R,
    sink: &mut S,
    limits: &Limits,
) -> Result<S::Value, Failure<S::Error>>
where
    R: EventReader<'input>,
    S: Sink,
{
    let mut executor = Executor {
        marker: std::marker::PhantomData,
        plan,
        reader,
        sink,
        limits,
        issues: Vec::new(),
    };
    let outcome = executor.run();
    let issues = executor.issues;
    let report = |kind, truncated| {
        Failure::Invalid(Report {
            kind,
            issues,
            truncated,
        })
    };
    match outcome {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(report(ReportKind::Validation, false)),
        Err(Stop::TooManyErrors) => Err(report(ReportKind::Validation, true)),
        Err(Stop::Unreadable) => Err(report(ReportKind::Decode, false)),
        Err(Stop::Sink(error)) => Err(Failure::Sink(error)),
    }
}

/// Why a run stops before the end of the input.
pub(crate) enum Stop<SinkError> {
    /// The input cannot be read further; the issue is recorded.
    Unreadable,
    TooManyErrors,
    Sink(SinkError),
}

/// The value of one step, or `None` when the input did not match and the issue is recorded.
pub(crate) type Step<S> = Result<Option<<S as Sink>::Value>, Stop<<S as Sink>::Error>>;

pub(crate) struct Executor<'run, 'input, R, S> {
    marker: std::marker::PhantomData<&'input ()>,
    plan: &'run Plan,
    reader: &'run mut R,
    sink: &'run mut S,
    limits: &'run Limits,
    issues: Vec<Issue>,
}

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    fn run(&mut self) -> Step<S> {
        let event = self.next_event()?;
        let value = self.value(self.plan.root(), event)?;
        self.next_event()?;
        Ok(value)
    }

    /// Reads the value that starts with `event` as the node `node_id`.
    #[inline]
    pub(crate) fn value(&mut self, node_id: NodeId, event: Event<'input>) -> Step<S> {
        let plan = self.plan;
        match plan.node(node_id) {
            Node::Any => self.any(event),
            Node::Bool => match event {
                Event::Bool(value) => self.emit(|sink| sink.bool(value)),
                other => self.mismatch("bool", other),
            },
            Node::None => match event {
                Event::Null => self.emit(Sink::none),
                other => self.mismatch("None", other),
            },
            Node::Integer(checks) => self.integer(checks, event),
            Node::Float(checks) => self.float(checks, event),
            Node::String(checks) => self.string(checks, event),
            Node::Bytes { mutable, length } => self.bytes(*mutable, *length, event),
            Node::Temporal(kind) => self.temporal(*kind, event),
            Node::Converted(conversion) => self.converted(*conversion, event),
            Node::List {
                kind,
                items,
                length,
            } => self.list(*kind, Some(*items), *length, event),
            Node::Tuple(items) => self.tuple(items, event),
            Node::Choice(choice) => self.choice(*choice, event),
            Node::Dict { values, length } => self.dict(Some(*values), *length, event),
            Node::Optional(inner) => match event {
                Event::Null => self.emit(Sink::none),
                other => self.value(*inner, other),
            },
            Node::Struct(structure) => self.structure(*structure, event),
            Node::Union(union) => self.union(union, event),
            Node::Tagged(tagged) => self.tagged(tagged, event),
            Node::Untagged(variants) => self.untagged(variants, event),
        }
    }

    #[inline]
    pub(crate) fn next_event(&mut self) -> Result<Event<'input>, Stop<S::Error>> {
        self.reader
            .next_event()
            .map_err(|error| self.unreadable(error))
    }

    #[inline]
    pub(crate) fn emit(
        &mut self,
        build: impl FnOnce(&mut S) -> Result<S::Value, S::Error>,
    ) -> Step<S> {
        build(self.sink).map(Some).map_err(Stop::Sink)
    }

    /// Records an issue at the current path.
    pub(crate) fn report(
        &mut self,
        code: ErrorCode,
        message: String,
        input: Option<String>,
        context: Vec<(&'static str, ContextValue)>,
    ) -> Result<(), Stop<S::Error>> {
        let path = Vec::new();
        self.issues.push(Issue {
            code,
            path,
            message,
            input,
            context,
        });
        if self.issues.len() >= self.limits.max_errors.max(1) {
            return Err(Stop::TooManyErrors);
        }
        Ok(())
    }

    /// Records that `event` is not the expected type and skips the rest of its value.
    pub(crate) fn mismatch(&mut self, expected: &str, event: Event<'input>) -> Step<S> {
        let received = received_type(&event);
        self.report(
            ErrorCode::Type,
            format!("expected {expected}, received {received}"),
            input_preview(&event),
            vec![
                ("expected", ContextValue::Text(expected.into())),
                ("received", ContextValue::Text(received.into())),
            ],
        )?;
        self.skip_value(event)?;
        Ok(None)
    }

    /// Records an exceeded limit; reading stops.
    pub(crate) fn limit_exceeded(
        &mut self,
        limit_name: &'static str,
        limit: usize,
        what: &str,
    ) -> Stop<S::Error> {
        let path = Vec::new();
        self.issues.push(Issue {
            code: ErrorCode::LimitExceeded,
            path,
            message: format!("{what} exceeds the limit {limit_name}={limit}"),
            input: None,
            context: vec![
                ("limit", ContextValue::Text(limit_name.into())),
                (
                    "value",
                    ContextValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX)),
                ),
            ],
        });
        Stop::Unreadable
    }

    fn unreadable(&mut self, error: ReaderError) -> Stop<S::Error> {
        let mut context = vec![
            (
                "line",
                ContextValue::Integer(i64::try_from(error.line).unwrap_or(i64::MAX)),
            ),
            (
                "column",
                ContextValue::Integer(i64::try_from(error.column).unwrap_or(i64::MAX)),
            ),
        ];
        let code = match error.kind.clone() {
            ReaderErrorKind::Syntax => ErrorCode::Syntax,
            ReaderErrorKind::DepthLimit { limit } => {
                context.push(("limit", ContextValue::Text("max_depth".into())));
                context.push((
                    "value",
                    ContextValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX)),
                ));
                ErrorCode::LimitExceeded
            }
        };
        let message = format!(
            "{} (line {}, column {})",
            error.message, error.line, error.column
        );
        let path = Vec::new();
        self.issues.push(Issue {
            code,
            path,
            message,
            input: None,
            context,
        });
        Stop::Unreadable
    }

    /// Puts `segment` in front of the path of every issue recorded since `mark`. Issues are
    /// recorded where they happen with an empty path, and each enclosing container adds its
    /// step on the way out, so reading valid input never builds a path.
    pub(crate) fn prefix_issues(&mut self, mark: usize, segment: impl FnOnce() -> PathSegment) {
        if self.issues.len() > mark {
            let segment = segment();
            for issue in &mut self.issues[mark..] {
                issue.path.insert(0, segment.clone());
            }
        }
    }

    /// Consumes the rest of the value that starts with `event`.
    pub(crate) fn skip_value(&mut self, event: Event<'input>) -> Result<(), Stop<S::Error>> {
        let mut depth: usize = match event {
            Event::MapStart | Event::SequenceStart => 1,
            _ => return Ok(()),
        };
        while depth > 0 {
            match self.next_event()? {
                Event::MapStart | Event::SequenceStart => depth += 1,
                Event::MapEnd | Event::SequenceEnd => depth -= 1,
                _ => {}
            }
        }
        Ok(())
    }
}

/// The Python name of what `event` starts.
fn received_type(event: &Event<'_>) -> &'static str {
    match event {
        Event::MapStart => "dict",
        Event::SequenceStart => "list",
        Event::String(_) | Event::Key(_) => "str",
        Event::Number(text) if numbers::is_integer_text(text) => "int",
        Event::Number(_) => "float",
        Event::Bool(_) => "bool",
        Event::Null => "None",
        Event::MapEnd | Event::SequenceEnd | Event::End => "nothing",
    }
}

/// The received value as text for an issue, truncated; `None` for containers.
pub(crate) fn input_preview(event: &Event<'_>) -> Option<String> {
    let text: &str = match event {
        Event::String(text) | Event::Key(text) => text,
        Event::Number(text) => text,
        Event::Bool(true) => "true",
        Event::Bool(false) => "false",
        Event::Null => "null",
        _ => return None,
    };
    Some(text.chars().take(INPUT_PREVIEW_CHARACTERS).collect())
}
