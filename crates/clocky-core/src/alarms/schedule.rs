//! Scheduling state of one alarm: when it rings next and what is pending.

use chrono::TimeZone;

use super::alarm_time::AlarmTime;
use super::error::AlarmError;
use super::firing::{Firing, classify, snooze_until};
use super::next_fire::next_fire;
use super::weekdays::Weekdays;
use crate::time::Timestamp;

/// How far back [`AlarmSchedule::pending`] looks for a repeating alarm. The latest
/// occurrence of a weekly alarm is never more than 7 days (plus a DST hour) ago.
const LOOKBACK_MILLIS: i64 = 8 * 24 * 60 * 60 * 1000;

/// The scheduling columns of an alarm (spec, section 3), with the rules that use them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlarmSchedule {
    /// Local time at which it rings.
    pub time: AlarmTime,
    /// Days on which it repeats; [`Weekdays::ONCE`] rings once.
    pub repeat: Weekdays,
    /// Whether it is active.
    pub enabled: bool,
    /// While snoozed, the instant at which it rings again.
    pub snoozed_until: Option<Timestamp>,
    /// The last occurrence that was handled (rang or was reported as missed).
    pub last_fired_at: Option<Timestamp>,
    /// When the alarm was created. Occurrences up to this instant are never pending.
    ///
    /// Reset it when the alarm is edited or re-enabled, so that occurrences from before the
    /// change are not reported as missed.
    pub created_at: Timestamp,
}

impl AlarmSchedule {
    /// A new, enabled alarm that has never rung and is not snoozed.
    #[must_use]
    pub fn new(time: AlarmTime, repeat: Weekdays, created_at: Timestamp) -> Self {
        Self {
            time,
            repeat,
            enabled: true,
            snoozed_until: None,
            last_fired_at: None,
            created_at,
        }
    }

    /// The next instant at which the alarm rings, looking from `after`.
    ///
    /// `None` if it is disabled. A snooze takes precedence over the regular schedule, and is
    /// returned even if it has already expired (see [`Self::pending`]).
    #[must_use]
    pub fn due_at<Tz: TimeZone>(&self, after: Timestamp, tz: &Tz) -> Option<Timestamp> {
        if !self.enabled {
            return None;
        }
        self.snoozed_until
            .or_else(|| next_fire(self.time, self.repeat, after, tz))
    }

    /// The state after the occurrence due at `at` was handled: it rang, or it was reported
    /// as missed.
    ///
    /// Clears the snooze, records `last_fired_at` and disables a once-alarm. Call it for
    /// [`Firing::Missed`] too, otherwise [`Self::pending`] keeps reporting it.
    #[must_use]
    pub fn after_fired(self, at: Timestamp) -> Self {
        Self {
            enabled: self.enabled && !self.repeat.is_once(),
            snoozed_until: None,
            last_fired_at: Some(at),
            ..self
        }
    }

    /// The state after the user snoozed the ringing alarm at `now` for `minutes`.
    ///
    /// Re-enables a once-alarm, so that it still rings at the end of the snooze after
    /// [`Self::after_fired`] disabled it. A repeating alarm keeps its `enabled` state: one
    /// the user disabled stays disabled.
    ///
    /// # Errors
    /// [`AlarmError::InvalidSnooze`] if `minutes` is zero.
    pub fn snooze(self, now: Timestamp, minutes: u32) -> Result<Self, AlarmError> {
        Ok(Self {
            enabled: self.enabled || self.repeat.is_once(),
            snoozed_until: Some(snooze_until(now, minutes)?),
            ..self
        })
    }

    /// The occurrence that came due by `now` and was not handled yet, with how to present it.
    ///
    /// Covers the app being closed or the computer suspended: it looks for occurrences
    /// after `last_fired_at` (or `created_at`) up to `now`, inclusive. When several were
    /// missed, only the most recent is reported. While snoozed, only the snooze counts.
    #[must_use]
    pub fn pending<Tz: TimeZone>(&self, now: Timestamp, tz: &Tz) -> Option<(Timestamp, Firing)> {
        if !self.enabled {
            return None;
        }
        let due = match self.snoozed_until {
            Some(until) => Some(until).filter(|until| *until <= now),
            None => self.latest_due(now, tz),
        }?;
        Some((due, classify(due, now)))
    }

    /// The most recent regular occurrence after the last handled one and up to `now`.
    fn latest_due<Tz: TimeZone>(&self, now: Timestamp, tz: &Tz) -> Option<Timestamp> {
        let handled = self
            .last_fired_at
            .map_or(self.created_at, |fired| fired.max(self.created_at));
        let due_after =
            |after| next_fire(self.time, self.repeat, after, tz).filter(|due| *due <= now);
        if self.repeat.is_once() {
            // A once-alarm only had one occurrence, however long ago it was.
            return due_after(handled);
        }
        let window_start = Timestamp::from_millis(now.as_millis().saturating_sub(LOOKBACK_MILLIS));
        let mut latest = None;
        let mut cursor = handled.max(window_start);
        while let Some(due) = due_after(cursor) {
            latest = Some(due);
            cursor = due;
        }
        latest
    }
}

#[cfg(test)]
mod tests;
