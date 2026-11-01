//! Reads the text of dates, times and durations. Fractions of a second beyond microseconds
//! are truncated, as Python's `fromisoformat` does.

use super::{Date, Duration, MICROSECONDS_PER_SECOND, SECONDS_PER_DAY, Time};

/// Why text is not the expected time type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalError {
    /// Not in the format, or out of range.
    Invalid,
    /// A `datetime` without the offset it requires.
    MissingOffset,
    /// An `ox.NaiveDateTime` with an offset.
    UnexpectedOffset,
}

/// `YYYY-MM-DD`.
pub fn parse_date(text: &str) -> Result<Date, TemporalError> {
    date(text.as_bytes()).ok_or(TemporalError::Invalid)
}

/// `HH:MM:SS[.ffffff]`, with an optional offset: `Z` or `±HH:MM`.
pub fn parse_time(text: &str) -> Result<Time, TemporalError> {
    time(text.as_bytes()).ok_or(TemporalError::Invalid)
}

/// A date and a time separated by `T` or a space. With `aware`, the time must have an offset;
/// without it, it must not.
pub fn parse_datetime(text: &str, aware: bool) -> Result<(Date, Time), TemporalError> {
    let bytes = text.as_bytes();
    let (Some(date_part), Some(b'T' | b't' | b' ')) = (bytes.get(..10), bytes.get(10)) else {
        return Err(TemporalError::Invalid);
    };
    let date = date(date_part).ok_or(TemporalError::Invalid)?;
    let time = time(&bytes[11..]).ok_or(TemporalError::Invalid)?;
    match (aware, time.offset) {
        (true, None) => Err(TemporalError::MissingOffset),
        (false, Some(_)) => Err(TemporalError::UnexpectedOffset),
        _ => Ok((date, time)),
    }
}

/// `[±]P[nW][nD][T[nH][nM][n[.f]S]]`. Years and months have no fixed length, so they are not
/// accepted.
pub fn parse_duration(text: &str) -> Result<Duration, TemporalError> {
    duration(text.as_bytes()).ok_or(TemporalError::Invalid)
}

fn date(bytes: &[u8]) -> Option<Date> {
    let [y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2] = *bytes else {
        return None;
    };
    let year = u16::try_from(number(&[y1, y2, y3, y4])?).ok()?;
    let month = u8::try_from(number(&[m1, m2])?).ok()?;
    let day = u8::try_from(number(&[d1, d2])?).ok()?;
    let valid = (1..=9999).contains(&year)
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day);
    valid.then_some(Date { year, month, day })
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn time(bytes: &[u8]) -> Option<Time> {
    let [h1, h2, b':', m1, m2, b':', s1, s2, ref rest @ ..] = *bytes else {
        return None;
    };
    let hour = u8::try_from(number(&[h1, h2])?).ok()?;
    let minute = u8::try_from(number(&[m1, m2])?).ok()?;
    let second = u8::try_from(number(&[s1, s2])?).ok()?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let (microsecond, rest) = match rest {
        [b'.', fraction @ ..] => {
            let digits = fraction
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digits == 0 {
                return None;
            }
            (microseconds(&fraction[..digits]), &fraction[digits..])
        }
        rest => (0, rest),
    };
    Some(Time {
        hour,
        minute,
        second,
        microsecond,
        offset: offset(rest)?,
    })
}

/// The offset in seconds: `Some(None)` when there is none, `None` when it is malformed.
fn offset(bytes: &[u8]) -> Option<Option<i32>> {
    let (sign, rest) = match bytes {
        [] => return Some(None),
        [b'Z' | b'z'] => return Some(Some(0)),
        [b'+', rest @ ..] => (1, rest),
        [b'-', rest @ ..] => (-1, rest),
        _ => return None,
    };
    let (hours, minutes, seconds) = match *rest {
        [h1, h2, b':', m1, m2] => (number(&[h1, h2])?, number(&[m1, m2])?, 0),
        [h1, h2, b':', m1, m2, b':', s1, s2] => {
            (number(&[h1, h2])?, number(&[m1, m2])?, number(&[s1, s2])?)
        }
        _ => return None,
    };
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let total = i32::try_from(hours * 3600 + minutes * 60 + seconds).ok()?;
    Some(Some(sign * total))
}

fn duration(bytes: &[u8]) -> Option<Duration> {
    let (negative, rest) = match bytes {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        rest => (false, rest),
    };
    let [b'P', rest @ ..] = rest else {
        return None;
    };
    let mut rest: &[u8] = rest;
    // The designators allowed, in the order they must come. `T` separates the date part.
    const DESIGNATORS: [(u8, i128); 5] = [
        (b'W', 7 * SECONDS_PER_DAY),
        (b'D', SECONDS_PER_DAY),
        (b'H', 3600),
        (b'M', 60),
        (b'S', 1),
    ];
    let mut next = 0;
    let mut in_time = false;
    let mut components = 0;
    let mut total: i128 = 0;
    while !rest.is_empty() {
        if let [b'T', after @ ..] = rest {
            if in_time || after.is_empty() {
                return None;
            }
            in_time = true;
            next = next.max(2);
            rest = after;
            continue;
        }
        let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        if digits == 0 || digits > 18 {
            return None;
        }
        let whole = i128::from(number(&rest[..digits])?);
        rest = &rest[digits..];
        let mut fraction = 0;
        if let [b'.', after @ ..] = rest {
            let fraction_digits = after
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if fraction_digits == 0 {
                return None;
            }
            fraction = i128::from(microseconds(&after[..fraction_digits]));
            rest = &after[fraction_digits..];
        }
        let [designator, after @ ..] = rest else {
            return None;
        };
        let position = DESIGNATORS[next..]
            .iter()
            .position(|(name, _)| name == designator)?
            + next;
        // `W` and `D` belong before `T`, `H`, `M` and `S` after it; only seconds take a fraction.
        if (position >= 2) != in_time || (fraction != 0 && *designator != b'S') {
            return None;
        }
        total += (whole * DESIGNATORS[position].1) * MICROSECONDS_PER_SECOND + fraction;
        next = position + 1;
        components += 1;
        rest = after;
    }
    if components == 0 || (in_time && next < 2) {
        return None;
    }
    Duration::from_microseconds(if negative { -total } else { total })
}

/// The microseconds of the digits after a decimal point: the first six, padded with zeros.
fn microseconds(digits: &[u8]) -> u32 {
    digits
        .iter()
        .chain(std::iter::repeat(&b'0'))
        .take(6)
        .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
}

/// The value of ASCII digits, or `None` when one is not a digit.
fn number(digits: &[u8]) -> Option<u64> {
    digits.iter().try_fold(0u64, |value, digit| {
        digit
            .is_ascii_digit()
            .then(|| value * 10 + u64::from(digit - b'0'))
    })
}
