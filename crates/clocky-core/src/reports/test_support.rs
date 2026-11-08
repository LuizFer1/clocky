//! Helpers for the report tests: local dates and instants, and recorded sessions.

use std::time::Duration;

use chrono::{NaiveDate, TimeZone};

use crate::domain::{SessionId, TaskId, TimerId};
use crate::engine::run::Segment;
use crate::engine::session::{CompletedSession, TimerKind};
use crate::store::Store;
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

/// Duration of `n` minutes.
pub(super) fn min(n: u64) -> Duration {
    Duration::from_mins(n)
}

/// Run id used by [`record`].
pub(super) const RUN: TimerId = TimerId(999);

/// Records a stopwatch session of `task` with one segment per `(start, end)` pair.
pub(super) fn record(
    store: &mut Store,
    task: Option<TaskId>,
    spans: &[(Timestamp, Timestamp)],
) -> SessionId {
    record_run(store, task, (RUN, None), TimerKind::Stopwatch, spans)
}

/// Records a session of timer `run` (with its pomodoro `round`) and the given kind.
pub(super) fn record_run(
    store: &mut Store,
    task: Option<TaskId>,
    (run, round): (TimerId, Option<u32>),
    kind: TimerKind,
    spans: &[(Timestamp, Timestamp)],
) -> SessionId {
    let segments = spans
        .iter()
        .map(|&(start, end)| Segment { start, end })
        .collect();
    let session = CompletedSession::new(kind, segments).unwrap();
    store.record_session(task, run, round, &session).unwrap()
}
