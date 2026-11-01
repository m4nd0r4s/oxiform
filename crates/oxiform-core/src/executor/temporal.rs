//! Dates, times and durations, read from their ISO text.

use super::{Executor, Step, input_preview};
use crate::description::TemporalKind;
use crate::events::{Event, EventReader};
use crate::issues::{ContextValue, ErrorCode};
use crate::sink::Sink;
use crate::temporal::{
    Temporal, TemporalError, parse_date, parse_datetime, parse_duration, parse_time,
};

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(crate) fn temporal(&mut self, kind: TemporalKind, event: Event<'input>) -> Step<S> {
        let Event::String(text) = &event else {
            return self.mismatch(kind.name(), event);
        };
        let parsed = match kind {
            TemporalKind::DateTime => parse_datetime(text, true).map(dated),
            TemporalKind::NaiveDateTime => parse_datetime(text, false).map(dated),
            TemporalKind::Date => parse_date(text).map(Temporal::Date),
            TemporalKind::Time => parse_time(text).map(Temporal::Time),
            TemporalKind::Duration => parse_duration(text).map(Temporal::Duration),
        };
        let (code, message, context) = match parsed {
            Ok(value) => return self.emit(|sink| sink.temporal(value)),
            Err(TemporalError::Invalid) => (
                ErrorCode::Format,
                format!("invalid {}", kind.format()),
                vec![("format", ContextValue::Text(kind.format().into()))],
            ),
            Err(TemporalError::MissingOffset) => (
                ErrorCode::Timezone,
                "a datetime needs a timezone; use ox.NaiveDateTime for one without".into(),
                Vec::new(),
            ),
            Err(TemporalError::UnexpectedOffset) => (
                ErrorCode::Timezone,
                "expected a datetime without a timezone".into(),
                Vec::new(),
            ),
        };
        self.report(code, message, input_preview(&event), context)?;
        Ok(None)
    }
}

fn dated((date, time): (crate::temporal::Date, crate::temporal::Time)) -> Temporal {
    Temporal::DateTime(date, time)
}
