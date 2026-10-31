//! `Literal` and `Enum`: a value from a fixed set.

use super::numbers::{Integer, is_integer_text, parse_integer};
use super::{Executor, Step, input_preview};
use crate::events::{Event, EventReader};
use crate::issues::{ContextValue, ErrorCode};
use crate::plan::ChoiceId;
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(crate) fn choice(&mut self, id: ChoiceId, event: Event<'input>) -> Step<S> {
        let plan = self.plan;
        let choice = plan.choice_plan(id);
        let index = match &event {
            Event::Null => choice.null,
            Event::Bool(flag) => choice.bools[usize::from(*flag)],
            Event::String(text) => choice.strings.get(text.as_ref()).copied(),
            Event::Number(text) if is_integer_text(text) => match parse_integer(text) {
                Integer::Small(number) => choice.integers.get(&number).copied(),
                Integer::Big(_) => None,
            },
            _ => None,
        };
        if let Some(index) = index {
            return self.emit(|sink| sink.choice(id, index));
        }
        let code = if choice.is_enum {
            ErrorCode::Enum
        } else {
            ErrorCode::Literal
        };
        self.report(
            code,
            format!("expected one of {}", choice.shown.join(", ")),
            input_preview(&event),
            vec![("allowed", ContextValue::Values(choice.values.clone()))],
        )?;
        self.skip_value(event)?;
        Ok(None)
    }
}
