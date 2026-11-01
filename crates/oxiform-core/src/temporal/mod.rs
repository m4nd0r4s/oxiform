//! Dates, times and durations as text formats carry them: RFC 3339 for `datetime`, ISO 8601
//! for `date` and `time`, and ISO 8601 durations (`P1DT2H`) for `timedelta`.

mod parse;

use std::fmt::Write;

pub use parse::{TemporalError, parse_date, parse_datetime, parse_duration, parse_time};

const MICROSECONDS_PER_SECOND: i128 = 1_000_000;
const SECONDS_PER_DAY: i128 = 86_400;

/// The largest number of days in a Python `timedelta`.
pub const MAX_DURATION_DAYS: i32 = 999_999_999;

/// A calendar date, already checked: years 1 to 9999, as Python allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// A time of day, with the offset from UTC in seconds when it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Time {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub microsecond: u32,
    pub offset: Option<i32>,
}

/// A duration as Python normalizes a `timedelta`: `seconds` below a day and `microseconds`
/// below a second are never negative, so only `days` carries the sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duration {
    pub days: i32,
    pub seconds: i32,
    pub microseconds: i32,
}

/// A value read as one of the time types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temporal {
    DateTime(Date, Time),
    Date(Date),
    Time(Time),
    Duration(Duration),
}

impl Duration {
    /// The duration of `total` microseconds, or `None` beyond the range of `timedelta`.
    pub fn from_microseconds(total: i128) -> Option<Self> {
        let per_day = SECONDS_PER_DAY * MICROSECONDS_PER_SECOND;
        let days = i32::try_from(total.div_euclid(per_day)).ok()?;
        if days.abs() > MAX_DURATION_DAYS {
            return None;
        }
        let rest = total.rem_euclid(per_day);
        Some(Self {
            days,
            seconds: i32::try_from(rest / MICROSECONDS_PER_SECOND).ok()?,
            microseconds: i32::try_from(rest % MICROSECONDS_PER_SECOND).ok()?,
        })
    }

    pub fn total_microseconds(self) -> i128 {
        (i128::from(self.days) * SECONDS_PER_DAY + i128::from(self.seconds))
            * MICROSECONDS_PER_SECOND
            + i128::from(self.microseconds)
    }
}

impl Temporal {
    /// The value as text: `2026-10-28T10:00:00Z`, `2026-10-28`, `10:00:00.5+01:00`, `P1DT2H`.
    pub fn write_iso(&self, out: &mut String) {
        match self {
            Self::DateTime(date, time) => {
                write_date(*date, out);
                out.push('T');
                write_time(*time, out);
            }
            Self::Date(date) => write_date(*date, out),
            Self::Time(time) => write_time(*time, out),
            Self::Duration(duration) => write_duration(*duration, out),
        }
    }
}

fn write_date(date: Date, out: &mut String) {
    let _ = write!(out, "{:04}-{:02}-{:02}", date.year, date.month, date.day);
}

/// Microseconds only when there are some, as Python's `isoformat` does; `Z` for UTC.
fn write_time(time: Time, out: &mut String) {
    let _ = write!(
        out,
        "{:02}:{:02}:{:02}",
        time.hour, time.minute, time.second
    );
    if time.microsecond != 0 {
        let _ = write!(out, ".{:06}", time.microsecond);
    }
    match time.offset {
        None => {}
        Some(0) => out.push('Z'),
        Some(offset) => {
            let sign = if offset < 0 { '-' } else { '+' };
            let offset = offset.unsigned_abs();
            let _ = write!(out, "{sign}{:02}:{:02}", offset / 3600, offset / 60 % 60);
            if offset % 60 != 0 {
                let _ = write!(out, ":{:02}", offset % 60);
            }
        }
    }
}

/// `[-]P[nD][T[nH][nM][n[.f]S]]`, leaving out zero parts; `P0D` for no time at all.
fn write_duration(duration: Duration, out: &mut String) {
    let total = duration.total_microseconds();
    if total < 0 {
        out.push('-');
    }
    out.push('P');
    let total = total.unsigned_abs();
    let micros_per_second = MICROSECONDS_PER_SECOND.unsigned_abs();
    let seconds = total / micros_per_second;
    let microseconds = total % micros_per_second;
    let days = seconds / 86_400;
    let (hours, minutes, seconds) = (seconds / 3600 % 24, seconds / 60 % 60, seconds % 60);
    if days != 0 {
        let _ = write!(out, "{days}D");
    }
    if hours == 0 && minutes == 0 && seconds == 0 && microseconds == 0 {
        if days == 0 {
            out.push_str("0D");
        }
        return;
    }
    out.push('T');
    if hours != 0 {
        let _ = write!(out, "{hours}H");
    }
    if minutes != 0 {
        let _ = write!(out, "{minutes}M");
    }
    if seconds != 0 || microseconds != 0 {
        let _ = write!(out, "{seconds}");
        if microseconds != 0 {
            let fraction = format!("{microseconds:06}");
            let _ = write!(out, ".{}", fraction.trim_end_matches('0'));
        }
        out.push('S');
    }
}

#[cfg(test)]
mod tests;
