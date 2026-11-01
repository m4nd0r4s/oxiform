use super::*;

fn iso(value: Temporal) -> String {
    let mut out = String::new();
    value.write_iso(&mut out);
    out
}

fn time(hour: u8, minute: u8, second: u8, microsecond: u32, offset: Option<i32>) -> Time {
    Time {
        hour,
        minute,
        second,
        microsecond,
        offset,
    }
}

#[test]
fn reads_dates_and_checks_the_calendar() {
    assert_eq!(
        parse_date("2024-02-29"),
        Ok(Date {
            year: 2024,
            month: 2,
            day: 29
        })
    );
    for invalid in [
        "2023-02-29",
        "1900-02-29",
        "0000-01-01",
        "2024-13-01",
        "2024-04-31",
        "2024-1-01",
        "2024/01/01",
        "2024-01-01T",
    ] {
        assert_eq!(
            parse_date(invalid),
            Err(TemporalError::Invalid),
            "{invalid}"
        );
    }
}

#[test]
fn reads_times_with_fractions_and_offsets() {
    assert_eq!(parse_time("10:20:30"), Ok(time(10, 20, 30, 0, None)));
    assert_eq!(
        parse_time("10:20:30.5Z"),
        Ok(time(10, 20, 30, 500_000, Some(0)))
    );
    assert_eq!(
        parse_time("23:59:59.1234567-03:30"),
        Ok(time(23, 59, 59, 123_456, Some(-12_600)))
    );
    for invalid in [
        "24:00:00",
        "10:60:00",
        "10:00:60",
        "10:00",
        "10:00:00.",
        "10:00:00+1",
    ] {
        assert_eq!(
            parse_time(invalid),
            Err(TemporalError::Invalid),
            "{invalid}"
        );
    }
}

#[test]
fn reads_datetimes_and_requires_the_offset_as_declared() {
    let (date, time) = parse_datetime("2026-10-28T10:00:00+01:00", true).expect("valid");
    assert_eq!((date.day, time.hour, time.offset), (28, 10, Some(3600)));
    assert!(parse_datetime("2026-10-28 10:00:00z", true).is_ok());
    assert_eq!(
        parse_datetime("2026-10-28T10:00:00", true),
        Err(TemporalError::MissingOffset)
    );
    assert_eq!(
        parse_datetime("2026-10-28T10:00:00Z", false),
        Err(TemporalError::UnexpectedOffset)
    );
    assert_eq!(
        parse_datetime("2026-10-28", true),
        Err(TemporalError::Invalid)
    );
}

#[test]
fn reads_durations() {
    let micros = |text: &str| parse_duration(text).map(Duration::total_microseconds);
    assert_eq!(micros("P1DT2H"), Ok((86_400 + 7200) * 1_000_000));
    assert_eq!(micros("PT0.25S"), Ok(250_000));
    assert_eq!(micros("-P2W"), Ok(-14 * 86_400 * 1_000_000));
    assert_eq!(micros("PT90M"), Ok(5400 * 1_000_000));
    assert_eq!(
        parse_duration("-PT1S"),
        Ok(Duration {
            days: -1,
            seconds: 86_399,
            microseconds: 0
        })
    );
    for invalid in [
        "P",
        "PT",
        "P1DT",
        "1D",
        "P1Y",
        "P1M",
        "PT1D",
        "P1H",
        "P1.5D",
        "PT1S2M",
        "P1D1D",
        "P1000000000D",
    ] {
        assert_eq!(
            parse_duration(invalid),
            Err(TemporalError::Invalid),
            "{invalid}"
        );
    }
}

#[test]
fn writes_iso_text() {
    let date = Date {
        year: 33,
        month: 1,
        day: 2,
    };
    assert_eq!(iso(Temporal::Date(date)), "0033-01-02");
    assert_eq!(
        iso(Temporal::DateTime(date, time(1, 2, 3, 0, Some(0)))),
        "0033-01-02T01:02:03Z"
    );
    assert_eq!(
        iso(Temporal::Time(time(1, 2, 3, 40, Some(-5400)))),
        "01:02:03.000040-01:30"
    );
    assert_eq!(
        iso(Temporal::Time(time(0, 0, 0, 0, Some(3661)))),
        "00:00:00+01:01:01"
    );
    let duration = |text: &str| iso(Temporal::Duration(parse_duration(text).expect("valid")));
    assert_eq!(duration("P1DT2H"), "P1DT2H");
    assert_eq!(duration("PT0S"), "P0D");
    assert_eq!(duration("-PT1.5S"), "-PT1.5S");
    assert_eq!(duration("P2W"), "P14D");
    assert_eq!(duration("PT25H"), "P1DT1H");
}
