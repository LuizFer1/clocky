//! Helpers for the report tests: local dates and instants in a given zone.

use chrono::{NaiveDate, TimeZone};

use crate::time::Timestamp;

/// The date `year-month-day`.
pub(super) fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

/// The instant at local `(year, month, day)` `(hour, minute)` in `tz` (the earliest one in a
/// fold).
pub(super) fn local<Tz: TimeZone>(
    tz: &Tz,
    (year, month, day): (i32, u32, u32),
    (hour, minute): (u32, u32),
) -> Timestamp {
    let dt = tz
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .earliest()
        .unwrap();
    Timestamp::from_millis(dt.timestamp_millis())
}
