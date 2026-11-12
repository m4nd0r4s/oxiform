//! Numbers and strings, with their constraints.

use std::cmp::Ordering;

use super::numbers::{
    Integer, compare_float, compare_integer, describe_bound, digit_count, is_integer_text,
    is_lax_float_text, lax_bool_text, lax_integer_text, parse_integer, without_leading_zeros,
};
use super::{
    Executor, LAX_BOOL_HINT, LAX_FLOAT_HINT, LAX_INTEGER_HINT, Step, Stop, input_preview,
    preview_text,
};
use crate::description::Bound;
use crate::events::{Event, EventReader};
use crate::issues::{ContextValue, ErrorCode};
use crate::options::Mode;
use crate::plan::{LengthRange, NumberChecks, StringChecks};
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(crate) fn integer(&mut self, checks: &NumberChecks, event: Event<'input>) -> Step<S> {
        match event {
            Event::Number(text) if is_integer_text(text) => self.integer_text(checks, text, text),
            other => self.integer_from_other(checks, other),
        }
    }

    /// An integer given as anything but integer text: an error, or in lax mode a string or a
    /// number such as `42.0` that holds one.
    #[cold]
    fn integer_from_other(&mut self, checks: &NumberChecks, event: Event<'input>) -> Step<S> {
        if self.mode == Mode::Lax {
            let text = match &event {
                Event::Number(text) => Some(*text),
                Event::String(text) => Some(text.as_ref()),
                _ => None,
            };
            if let Some(received) = text
                && let Some(integer) = lax_integer_text(received)
            {
                return self.integer_text(checks, integer, received);
            }
            return self.lax_mismatch("int", LAX_INTEGER_HINT, event);
        }
        self.mismatch("int", event)
    }

    /// An integer from `text`, its digits, with the text `received` that holds them (`"42.0"`
    /// for `42`), which is what an issue shows.
    fn integer_text(&mut self, checks: &NumberChecks, text: &str, received: &str) -> Step<S> {
        let value = self.read_integer(text)?;
        let input = || Some(preview_text(received));
        let mut valid = self.check_range(checks, |bound| compare_integer(value, bound), input)?;
        if let Some(allowed) = &checks.one_of
            && !matches!(value, Integer::Small(small) if allowed.contains(&small))
        {
            let listed = allowed
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            self.report(
                ErrorCode::Literal,
                format!("must be one of {listed}"),
                input(),
                vec![("allowed", ContextValue::Integers(allowed.clone()))],
            )?;
            valid = false;
        }
        if !valid {
            return Ok(None);
        }
        self.emit_integer(value)
    }

    pub(crate) fn float(&mut self, checks: &NumberChecks, event: Event<'input>) -> Step<S> {
        match event {
            Event::Number(text) => self.float_text(checks, text),
            other => self.float_from_other(checks, other),
        }
    }

    /// A float given as anything but a number: an error, or in lax mode a string that holds one.
    #[cold]
    fn float_from_other(&mut self, checks: &NumberChecks, event: Event<'input>) -> Step<S> {
        if self.mode == Mode::Lax
            && let Event::String(text) = &event
            && is_lax_float_text(text)
        {
            return self.float_text(checks, text);
        }
        if self.mode == Mode::Lax {
            return self.lax_mismatch("float", LAX_FLOAT_HINT, event);
        }
        self.mismatch("float", event)
    }

    fn float_text(&mut self, checks: &NumberChecks, text: &str) -> Step<S> {
        let Some(value) = self.read_float(text)? else {
            return Ok(None);
        };
        let valid = self.check_range(
            checks,
            |bound| compare_float(value, bound),
            || Some(preview_text(text)),
        )?;
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.float(value))
    }

    /// A boolean: `true` or `false`, and in lax mode the words that mean them (`yes`, `0`).
    pub(crate) fn boolean(&mut self, event: Event<'input>) -> Step<S> {
        match event {
            Event::Bool(value) => self.emit(|sink| sink.bool(value)),
            other => self.boolean_from_other(other),
        }
    }

    #[cold]
    fn boolean_from_other(&mut self, event: Event<'input>) -> Step<S> {
        if self.mode == Mode::Lax
            && let Event::String(text) = &event
            && let Some(value) = lax_bool_text(text)
        {
            return self.emit(|sink| sink.bool(value));
        }
        if self.mode == Mode::Lax {
            return self.lax_mismatch("bool", LAX_BOOL_HINT, event);
        }
        self.mismatch("bool", event)
    }

    pub(crate) fn string(&mut self, checks: &StringChecks, event: Event<'input>) -> Step<S> {
        let Event::String(text) = &event else {
            return self.mismatch("str", event);
        };
        let mut valid = true;
        if !checks.length.is_unbounded() {
            valid &= self.check_length(checks.length, text.chars().count(), || {
                input_preview(&event)
            })?;
        }
        if let Some(pattern) = &checks.pattern
            && !pattern.is_match(text)
        {
            self.report(
                ErrorCode::Pattern,
                format!("does not match the pattern {:?}", pattern.as_str()),
                input_preview(&event),
                vec![("pattern", ContextValue::Text(pattern.as_str().into()))],
            )?;
            valid = false;
        }
        if let Some(allowed) = &checks.one_of
            && !allowed.iter().any(|choice| choice == text)
        {
            let listed = allowed
                .iter()
                .map(|choice| format!("{choice:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.report(
                ErrorCode::Literal,
                format!("must be one of {listed}"),
                input_preview(&event),
                vec![("allowed", ContextValue::Texts(allowed.clone()))],
            )?;
            valid = false;
        }
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.string(text))
    }

    /// Reads integer text, enforcing the digit limit.
    pub(crate) fn read_integer<'text>(
        &mut self,
        text: &'text str,
    ) -> Result<Integer<'text>, Stop<S::Error>> {
        let digits = digit_count(text);
        if digits > self.limits.max_int_digits {
            return Err(self.limit_exceeded(
                "max_int_digits",
                self.limits.max_int_digits,
                "the integer",
            ));
        }
        // An integer of at most 18 digits fits 64 bits and never reaches the sink's own limit.
        if digits > 18
            && let Some((limit, hint)) = self.sink.integer_digit_limit()
            && digits > limit
        {
            let message = format!(
                "the integer exceeds the limit max_int_digits={limit}, the most this runtime \
                 reads ({hint})"
            );
            return Err(self.limit_exceeded_with("max_int_digits", limit, message));
        }
        Ok(parse_integer(text))
    }

    /// Reads number text as a float; out-of-range numbers are a type issue.
    #[inline]
    pub(crate) fn read_float(&mut self, text: &str) -> Result<Option<f64>, Stop<S::Error>> {
        let value: f64 = text.parse().unwrap_or(f64::INFINITY);
        if value.is_finite() {
            return Ok(Some(value));
        }
        self.report(
            ErrorCode::Type,
            "number out of range for float".into(),
            Some(text.chars().take(80).collect()),
            vec![("expected", ContextValue::Text("float".into()))],
        )?;
        Ok(None)
    }

    #[inline]
    pub(crate) fn emit_integer(&mut self, value: Integer<'_>) -> Step<S> {
        match value {
            Integer::Small(small) => self.emit(|sink| sink.integer(small)),
            Integer::Big(digits) => {
                let digits = without_leading_zeros(digits);
                self.emit(|sink| sink.big_integer(&digits))
            }
        }
    }

    /// Checks `Ge` and `Le`. Returns whether the value passed.
    fn check_range(
        &mut self,
        checks: &NumberChecks,
        compare: impl Fn(Bound) -> Ordering,
        input: impl Fn() -> Option<String>,
    ) -> Result<bool, Stop<S::Error>> {
        let mut valid = true;
        if let Some(minimum) = checks.minimum
            && compare(minimum) == Ordering::Less
        {
            self.report(
                ErrorCode::TooSmall,
                format!("must be at least {}", describe_bound(minimum)),
                input(),
                vec![
                    ("limit", bound_context(minimum)),
                    ("inclusive", ContextValue::Bool(true)),
                ],
            )?;
            valid = false;
        }
        if let Some(maximum) = checks.maximum
            && compare(maximum) == Ordering::Greater
        {
            self.report(
                ErrorCode::TooLarge,
                format!("must be at most {}", describe_bound(maximum)),
                input(),
                vec![
                    ("limit", bound_context(maximum)),
                    ("inclusive", ContextValue::Bool(true)),
                ],
            )?;
            valid = false;
        }
        Ok(valid)
    }

    /// Checks `Len`. Returns whether the length passed.
    pub(crate) fn check_length(
        &mut self,
        range: LengthRange,
        actual: usize,
        input: impl Fn() -> Option<String>,
    ) -> Result<bool, Stop<S::Error>> {
        let context = || {
            let as_context = |value: Option<usize>| match value {
                Some(value) => ContextValue::Integer(i64::try_from(value).unwrap_or(i64::MAX)),
                None => ContextValue::Text("none".into()),
            };
            vec![
                ("min", as_context(range.minimum)),
                ("max", as_context(range.maximum)),
                ("actual", as_context(Some(actual))),
            ]
        };
        if let Some(minimum) = range.minimum
            && actual < minimum
        {
            self.report(
                ErrorCode::TooShort,
                format!("length {actual} is less than the minimum {minimum}"),
                input(),
                context(),
            )?;
            return Ok(false);
        }
        if let Some(maximum) = range.maximum
            && actual > maximum
        {
            self.report(
                ErrorCode::TooLong,
                format!("length {actual} is more than the maximum {maximum}"),
                input(),
                context(),
            )?;
            return Ok(false);
        }
        Ok(true)
    }
}

fn bound_context(bound: Bound) -> ContextValue {
    match bound {
        Bound::Integer(value) => ContextValue::Integer(value),
        Bound::Float(value) => ContextValue::Float(value),
    }
}
