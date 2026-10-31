//! Numbers and strings, with their constraints.

use std::cmp::Ordering;

use super::numbers::{
    Integer, compare_float, compare_integer, describe_bound, digit_count, is_integer_text,
    parse_integer,
};
use super::{Executor, Step, Stop, input_preview};
use crate::description::Bound;
use crate::events::{Event, EventReader};
use crate::issues::{ContextValue, ErrorCode};
use crate::plan::{LengthRange, NumberChecks, StringChecks};
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(crate) fn integer(&mut self, checks: &NumberChecks, event: Event<'input>) -> Step<S> {
        let Event::Number(text) = event else {
            return self.mismatch("int", event);
        };
        if !is_integer_text(text) {
            return self.mismatch("int", event);
        }
        let value = self.read_integer(text)?;
        let input = || Some(text.to_owned());
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
        let Event::Number(text) = event else {
            return self.mismatch("float", event);
        };
        let Some(value) = self.read_float(text)? else {
            return Ok(None);
        };
        let valid = self.check_range(
            checks,
            |bound| compare_float(value, bound),
            || Some(text.to_owned()),
        )?;
        if !valid {
            return Ok(None);
        }
        self.emit(|sink| sink.float(value))
    }

    pub(crate) fn string(&mut self, checks: &StringChecks, event: Event<'input>) -> Step<S> {
        let Event::String(text) = &event else {
            return self.mismatch("str", event);
        };
        self.check_string_size(text)?;
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
        if digit_count(text) > self.limits.max_int_digits {
            return Err(self.limit_exceeded(
                "max_int_digits",
                self.limits.max_int_digits,
                "the integer",
            ));
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
            Integer::Big(digits) => self.emit(|sink| sink.big_integer(digits)),
        }
    }

    #[inline]
    pub(crate) fn check_string_size(&mut self, text: &str) -> Result<(), Stop<S::Error>> {
        if text.len() > self.limits.max_str_len {
            return Err(self.limit_exceeded("max_str_len", self.limits.max_str_len, "the string"));
        }
        Ok(())
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
