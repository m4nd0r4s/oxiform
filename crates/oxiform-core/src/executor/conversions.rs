//! Bytes and conversions: values read from a string or a number that the sink turns into
//! another type.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::numbers::is_integer_text;
use super::{Executor, Step, Stop, input_preview};
use crate::description::ConversionInput;
use crate::events::{Event, EventReader};
use crate::issues::{ContextValue, ErrorCode};
use crate::plan::{ConversionId, LengthRange};
use crate::sink::Sink;

impl<'input, R: EventReader<'input>, S: Sink> Executor<'_, 'input, R, S> {
    pub(crate) fn bytes(
        &mut self,
        mutable: bool,
        length: LengthRange,
        event: Event<'input>,
    ) -> Step<S> {
        let Event::String(text) = &event else {
            let expected = if mutable { "bytearray" } else { "bytes" };
            return self.mismatch(expected, event);
        };
        self.check_string_size(text)?;
        let Ok(decoded) = STANDARD.decode(text.as_bytes()) else {
            self.report_format("base64", "invalid base64".into(), &event)?;
            return Ok(None);
        };
        if !length.is_unbounded()
            && !self.check_length(length, decoded.len(), || input_preview(&event))?
        {
            return Ok(None);
        }
        self.emit(|sink| sink.bytes(&decoded, mutable))
    }

    pub(crate) fn converted(&mut self, id: ConversionId, event: Event<'input>) -> Step<S> {
        let plan = self.plan;
        let conversion = plan.conversion(id);
        let raw = match (conversion.input, &event) {
            (ConversionInput::String, Event::String(text)) => {
                self.check_string_size(text)?;
                self.emit(|sink| sink.string(text))?
            }
            (ConversionInput::Integer, Event::Number(text)) if is_integer_text(text) => {
                let value = self.read_integer(text)?;
                self.emit_integer(value)?
            }
            (ConversionInput::Decimal, Event::Number(text)) => {
                self.emit(|sink| sink.string(text))?
            }
            (ConversionInput::Decimal, Event::String(text)) => {
                self.check_string_size(text)?;
                if !is_decimal_text(text) {
                    let message = format!("invalid {}", conversion.name);
                    self.report_format(&conversion.format, message, &event)?;
                    return Ok(None);
                }
                self.emit(|sink| sink.string(text))?
            }
            _ => return self.mismatch(&conversion.name, event),
        };
        let Some(raw) = raw else {
            return Ok(None);
        };
        let converted = self.sink.convert(id, raw).map_err(Stop::Sink)?;
        if converted.is_some() {
            return Ok(converted);
        }
        if conversion.input == ConversionInput::Integer {
            let shown = input_preview(&event).unwrap_or_default();
            self.report(
                ErrorCode::Enum,
                format!("{shown} is not a valid {}", conversion.name),
                input_preview(&event),
                Vec::new(),
            )?;
        } else {
            let message = format!("invalid {}", conversion.name);
            self.report_format(&conversion.format, message, &event)?;
        }
        Ok(None)
    }

    fn report_format(
        &mut self,
        format: &str,
        message: String,
        event: &Event<'input>,
    ) -> Result<(), Stop<S::Error>> {
        self.report(
            ErrorCode::Format,
            message,
            input_preview(event),
            vec![("format", ContextValue::Text(format.into()))],
        )
    }
}

/// Whether `text` is a decimal number as Python's `Decimal` reads it, without the surrounding
/// whitespace and digit separators it also tolerates: `-1.5`, `.5`, `2e10`, `Infinity`, `NaN`.
fn is_decimal_text(text: &str) -> bool {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if is_special_decimal(unsigned) {
        return true;
    }
    let bytes = unsigned.as_bytes();
    let integer_digits = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let mut rest = &bytes[integer_digits..];
    let mut fraction_digits = 0;
    if let [b'.', after @ ..] = rest {
        fraction_digits = after
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        rest = &after[fraction_digits..];
    }
    if integer_digits + fraction_digits == 0 {
        return false;
    }
    match rest {
        [] => true,
        [b'e' | b'E', exponent @ ..] => {
            let digits = match exponent {
                [b'+' | b'-', digits @ ..] => digits,
                digits => digits,
            };
            !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
        }
        _ => false,
    }
}

/// `Infinity`, `Inf`, `NaN` and `sNaN` in any case, with the optional diagnostic digits a NaN
/// can carry.
fn is_special_decimal(text: &str) -> bool {
    if text.eq_ignore_ascii_case("inf") || text.eq_ignore_ascii_case("infinity") {
        return true;
    }
    let after_signal = match text.as_bytes() {
        [b's' | b'S', rest @ ..] => rest,
        rest => rest,
    };
    match after_signal {
        [n, a, m, digits @ ..] => {
            [*n, *a, *m].map(|byte| byte.to_ascii_lowercase()) == *b"nan"
                && digits.iter().all(u8::is_ascii_digit)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_decimal_text;

    #[test]
    fn reads_decimal_text_as_python_does() {
        for valid in [
            "0", "-1.5", "+.5", "5.", "1e10", "1E-3", "Infinity", "-inf", "NaN", "snan12",
        ] {
            assert!(is_decimal_text(valid), "{valid}");
        }
        for invalid in [
            "", ".", "-", "1e", "1_000", " 1", "0x10", "1.5.2", "nano", "e5",
        ] {
            assert!(!is_decimal_text(invalid), "{invalid}");
        }
    }
}
