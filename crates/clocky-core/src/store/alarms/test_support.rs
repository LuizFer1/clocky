//! Helpers for the alarm store tests.

use chrono::{FixedOffset, TimeZone};

use super::NewAlarm;
use crate::alarms::{AlarmTime, Weekdays};
use crate::domain::{AlarmId, TaskId};
use crate::store::Store;
use crate::time::Timestamp;

/// The alarm time `hour:minute`.
pub(super) fn time(hour: u8, minute: u8) -> AlarmTime {
    AlarmTime::new(hour, minute).unwrap()
}

/// A weekday alarm at 07:30, snoozing for 5 minutes.
pub(super) fn wake(task: Option<TaskId>) -> NewAlarm<'static> {
    NewAlarm {
        name: "wake up",
        time: time(7, 30),
        repeat: Weekdays::WEEKDAYS,
        sound: None,
        message: "",
        snooze_minutes: 5,
        task,
    }
}

/// An alarm named `name` at `hour:minute`, otherwise like [`wake`].
pub(super) fn at_time(name: &'static str, hour: u8, minute: u8) -> NewAlarm<'static> {
    NewAlarm {
        name,
        time: time(hour, minute),
        ..wake(None)
    }
}

/// Brasília time (UTC-3), with no daylight saving time.
pub(super) fn brt() -> FixedOffset {
    FixedOffset::west_opt(3 * 3600).unwrap()
}

/// The instant at local `day` of November 2026 (the 2nd is a Monday), `hour:minute`, in BRT.
pub(super) fn nov(day: u32, hour: u32, minute: u32) -> Timestamp {
    let local = brt()
        .with_ymd_and_hms(2026, 11, day, hour, minute, 0)
        .unwrap();
    Timestamp::from_millis(local.timestamp_millis())
}

/// Creates a daily alarm at `hour:minute`, armed at 06:00 on November 2nd.
pub(super) fn daily(store: &mut Store, hour: u8, minute: u8) -> AlarmId {
    let alarm = NewAlarm {
        repeat: Weekdays::EVERY_DAY,
        ..at_time("daily", hour, minute)
    };
    store.create_alarm(&alarm, nov(2, 6, 0)).unwrap()
}
