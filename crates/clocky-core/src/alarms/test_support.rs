//! Helpers for the alarm tests.

use chrono::{FixedOffset, TimeZone, Utc};

use super::alarm_time::AlarmTime;
use crate::time::Timestamp;

/// Brasília time (UTC-3), with no daylight saving time.
pub(super) fn brt() -> FixedOffset {
    FixedOffset::west_opt(3 * 3600).unwrap()
}

/// The alarm time `hour:minute`.
pub(super) fn time(hour: u8, minute: u8) -> AlarmTime {
    AlarmTime::new(hour, minute).unwrap()
}

/// The instant at local `day` of November 2026 (the 2nd is a Monday), `hour:minute`, in `tz`.
pub(super) fn nov<Tz: TimeZone>(tz: &Tz, day: u32, hour: u32, minute: u32) -> Timestamp {
    let dt = tz.with_ymd_and_hms(2026, 11, day, hour, minute, 0).unwrap();
    Timestamp::from_millis(dt.timestamp_millis())
}

/// The instant at `hour:minute` UTC on the given date.
pub(super) fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Timestamp {
    let dt = Utc
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .unwrap();
    Timestamp::from_millis(dt.timestamp_millis())
}
