//! Number text: integers of any size, and comparisons with declared limits.

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

/// Number of digits, without the sign.
#[inline]
pub(crate) fn digit_count(text: &str) -> usize {
    text.trim_start_matches('-').len()
}

#[inline]
pub(crate) fn parse_integer(text: &str) -> Integer<'_> {
    text.parse().map_or(Integer::Big(text), Integer::Small)
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
