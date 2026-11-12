//! Number text: integers of any size, and comparisons with declared limits.

use std::borrow::Cow;
use std::cmp::Ordering;

use crate::description::Bound;

/// An integer read from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Integer<'text> {
    Small(i64),
    /// Outside the 64-bit range: the digits as written.
    Big(&'text str),
}

/// Whether the number text has no fraction and no exponent.
#[inline]
pub(crate) fn is_integer_text(text: &str) -> bool {
    !text.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E'))
}

/// Number of digits that count against `max_int_digits`: without the sign and without leading
/// zeros (`"000042"` has two), but at least one (`"0"`).
#[inline]
pub(crate) fn digit_count(text: &str) -> usize {
    let digits = text.trim_start_matches(['-', '+']);
    digits
        .trim_start_matches('0')
        .len()
        .max(1)
        .min(digits.len())
}

/// `text` without the zeros in front of its digits (`"-007"` is `"-7"`), as Python's `int`
/// counts them toward its own digit limit.
pub(crate) fn without_leading_zeros(text: &str) -> Cow<'_, str> {
    let (sign, digits) = match text.strip_prefix(['-', '+']) {
        Some(digits) => (&text[..1], digits),
        None => ("", text),
    };
    let significant = digits.trim_start_matches('0');
    if significant.len() == digits.len() || significant.is_empty() {
        return Cow::Borrowed(text);
    }
    if sign.is_empty() {
        Cow::Borrowed(significant)
    } else {
        Cow::Owned(format!("{sign}{significant}"))
    }
}

#[inline]
pub(crate) fn parse_integer(text: &str) -> Integer<'_> {
    text.parse().map_or(Integer::Big(text), Integer::Small)
}

/// The integer that lax mode reads from `text`: an optional sign and digits, and optionally a
/// fraction of zeros (`42`, `-7`, `+3`, `42.0`). The digits may have leading zeros, which do
/// not count as digits. Anything
/// that would lose information, such as `4.5` or `1e3`, is not an integer.
pub(crate) fn lax_integer_text(text: &str) -> Option<&str> {
    let (integer, fraction) = match text.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (text, None),
    };
    if fraction.is_some_and(|zeros| zeros.is_empty() || !zeros.bytes().all(|byte| byte == b'0')) {
        return None;
    }
    let digits = integer.strip_prefix(['-', '+']).unwrap_or(integer);
    let valid = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
    valid.then_some(integer)
}

/// Whether lax mode reads `text` as a float: an optional sign, digits with an optional
/// fraction, and an optional exponent (`1.5`, `-.5`, `2e10`). `inf` and `nan` are not numbers.
pub(crate) fn is_lax_float_text(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text).as_bytes();
    let integer_digits = unsigned
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let mut rest = &unsigned[integer_digits..];
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

/// The boolean that lax mode reads from `text`, in any case: `true`, `yes`, `on` and `1`, or
/// `false`, `no`, `off` and `0`.
pub(crate) fn lax_bool_text(text: &str) -> Option<bool> {
    const TRUE: [&str; 4] = ["true", "yes", "on", "1"];
    const FALSE: [&str; 4] = ["false", "no", "off", "0"];
    if TRUE.iter().any(|word| word.eq_ignore_ascii_case(text)) {
        Some(true)
    } else if FALSE.iter().any(|word| word.eq_ignore_ascii_case(text)) {
        Some(false)
    } else {
        None
    }
}

/// How `value` compares with `bound`.
pub(crate) fn compare_integer(value: Integer<'_>, bound: Bound) -> Ordering {
    match (value, bound) {
        (Integer::Small(value), Bound::Integer(bound)) => value.cmp(&bound),
        (Integer::Small(value), Bound::Float(bound)) => (value as f64).total_cmp(&bound),
        // A big integer is beyond every 64-bit bound: its sign decides.
        (Integer::Big(digits), Bound::Integer(_)) if digits.starts_with('-') => Ordering::Less,
        (Integer::Big(_), Bound::Integer(_)) => Ordering::Greater,
        (Integer::Big(digits), Bound::Float(bound)) => {
            digits.parse::<f64>().unwrap_or(f64::NAN).total_cmp(&bound)
        }
    }
}

/// How `value` compares with `bound`.
pub(crate) fn compare_float(value: f64, bound: Bound) -> Ordering {
    match bound {
        Bound::Integer(bound) => value.total_cmp(&(bound as f64)),
        Bound::Float(bound) => value.total_cmp(&bound),
    }
}

pub(crate) fn describe_bound(bound: Bound) -> String {
    match bound {
        Bound::Integer(value) => value.to_string(),
        Bound::Float(value) => format!("{value:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        digit_count, is_lax_float_text, lax_bool_text, lax_integer_text, without_leading_zeros,
    };

    #[test]
    fn counts_the_digits_of_the_value() {
        for (text, count) in [
            ("0", 1),
            ("-0", 1),
            ("000", 1),
            ("7", 1),
            ("007", 1),
            ("-0012", 2),
            ("+120", 3),
            ("1000", 4),
            ("-123456789012345678901234567890", 30),
        ] {
            assert_eq!(digit_count(text), count, "{text}");
        }
    }

    #[test]
    fn strips_leading_zeros_keeping_the_sign() {
        for (text, stripped) in [
            ("7", "7"),
            ("0", "0"),
            ("000", "000"),
            ("007", "7"),
            ("-007", "-7"),
            ("+0012", "12"),
            ("-100", "-100"),
        ] {
            assert_eq!(without_leading_zeros(text), stripped, "{text}");
        }
    }

    #[test]
    fn reads_lax_integers_without_losing_information() {
        for (text, integer) in [
            ("42", "42"),
            ("-7", "-7"),
            ("+3", "+3"),
            ("007", "007"),
            ("42.0", "42"),
            ("-1.000", "-1"),
        ] {
            assert_eq!(lax_integer_text(text), Some(integer), "{text}");
        }
        for text in [
            "", "-", "4.5", "1e3", "1_000", " 1", "1 ", "0x10", "4.", ".0", "١٢",
        ] {
            assert_eq!(lax_integer_text(text), None, "{text:?}");
        }
    }

    #[test]
    fn reads_lax_floats() {
        for text in ["1", "-1.5", "+.5", "5.", "2e10", "1E-3"] {
            assert!(is_lax_float_text(text), "{text}");
        }
        for text in ["", ".", "-", "e5", "1e", "inf", "nan", "1,5", " 1", "0x1"] {
            assert!(!is_lax_float_text(text), "{text:?}");
        }
    }

    #[test]
    fn reads_lax_booleans() {
        for text in ["true", "TRUE", "Yes", "on", "1"] {
            assert_eq!(lax_bool_text(text), Some(true), "{text}");
        }
        for text in ["false", "No", "OFF", "0"] {
            assert_eq!(lax_bool_text(text), Some(false), "{text}");
        }
        for text in ["", "2", "y", "t", "maybe"] {
            assert_eq!(lax_bool_text(text), None, "{text:?}");
        }
    }
}
