use oxiform_core::description::{TemporalKind, TypeDescription};
use oxiform_core::temporal::{Date, Temporal};

use crate::support::{Value, decode, plain, summary};

#[test]
fn reads_each_time_type_from_a_string() {
    let date = plain(TypeDescription::Temporal(TemporalKind::Date));
    assert_eq!(
        decode(&date, r#""2026-11-01""#),
        Ok(Value::Temporal(Temporal::Date(Date {
            year: 2026,
            month: 11,
            day: 1
        })))
    );
    assert_eq!(
        summary(&date, r#""2026-11-31""#),
        [("format", String::new(), "invalid date".into())]
    );
    assert_eq!(
        summary(&date, "20261101"),
        [("type", String::new(), "expected date, received int".into())]
    );
}

#[test]
fn reports_a_missing_or_unexpected_timezone() {
    let aware = plain(TypeDescription::Temporal(TemporalKind::DateTime));
    let naive = plain(TypeDescription::Temporal(TemporalKind::NaiveDateTime));
    assert!(decode(&aware, r#""2026-11-01T10:00:00Z""#).is_ok());
    assert!(decode(&naive, r#""2026-11-01T10:00:00""#).is_ok());
    assert_eq!(summary(&aware, r#""2026-11-01T10:00:00""#)[0].0, "timezone");
    assert_eq!(
        summary(&naive, r#""2026-11-01T10:00:00+01:00""#),
        [(
            "timezone",
            String::new(),
            "expected a datetime without a timezone".into()
        )]
    );
}
