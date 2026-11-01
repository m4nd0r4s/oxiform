//! Python dates, times and durations, read into the values the core writes as ISO text.

use oxiform_core::temporal::{Date, Duration, Temporal, Time};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDateAccess, PyDateTime, PyDelta, PyDeltaAccess, PyTime, PyTimeAccess};

/// The value of a `datetime`, `date`, `time` or `timedelta`, or `None` for any other object.
pub(super) fn temporal_of(object: &Bound<'_, PyAny>) -> PyResult<Option<Temporal>> {
    Ok(Some(if let Ok(datetime) = object.cast::<PyDateTime>() {
        Temporal::DateTime(date(datetime)?, time(datetime, utc_offset(object)?))
    } else if let Ok(day) = object.cast::<PyDate>() {
        Temporal::Date(date(day)?)
    } else if let Ok(clock) = object.cast::<PyTime>() {
        Temporal::Time(time(clock, utc_offset(object)?))
    } else if let Ok(delta) = object.cast::<PyDelta>() {
        Temporal::Duration(Duration {
            days: delta.get_days(),
            seconds: delta.get_seconds(),
            microseconds: delta.get_microseconds(),
        })
    } else {
        return Ok(None);
    }))
}

fn date(value: &impl PyDateAccess) -> PyResult<Date> {
    Ok(Date {
        year: u16::try_from(value.get_year())?,
        month: value.get_month(),
        day: value.get_day(),
    })
}

fn time(value: &impl PyTimeAccess, offset: Option<i32>) -> Time {
    Time {
        hour: value.get_hour(),
        minute: value.get_minute(),
        second: value.get_second(),
        microsecond: value.get_microsecond(),
        offset,
    }
}

/// The offset from UTC in seconds, from `utcoffset()`, which every `tzinfo` answers. `None`
/// for a naive value.
fn utc_offset(object: &Bound<'_, PyAny>) -> PyResult<Option<i32>> {
    let offset = object.call_method0(intern!(object.py(), "utcoffset"))?;
    if offset.is_none() {
        return Ok(None);
    }
    let offset = offset.cast_into::<PyDelta>()?;
    Ok(Some(offset.get_days() * 86_400 + offset.get_seconds()))
}
