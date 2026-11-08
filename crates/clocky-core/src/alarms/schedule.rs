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
    /// Re-enables the alarm, so that a once-alarm already disabled by
    /// [`Self::after_fired`] still rings at the end of the snooze.
    ///
    /// # Errors
    /// [`AlarmError::InvalidSnooze`] if `minutes` is zero.
    pub fn snooze(self, now: Timestamp, minutes: u32) -> Result<Self, AlarmError> {
        Ok(Self {
            enabled: true,
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
mod tests {
    use std::time::Duration;

    use chrono::Weekday;
    use chrono_tz::America::New_York;

    use super::*;
    use crate::alarms::test_support::{brt, nov, time, utc};
    use crate::test_util::mins;

    /// A daily 07:00 alarm created on Monday the 2nd at 06:00.
    fn daily() -> AlarmSchedule {
        AlarmSchedule::new(time(7, 0), Weekdays::EVERY_DAY, nov(&brt(), 2, 6, 0))
    }

    /// A once 07:00 alarm created on Monday the 2nd at 06:00.
    fn once() -> AlarmSchedule {
        AlarmSchedule::new(time(7, 0), Weekdays::ONCE, nov(&brt(), 2, 6, 0))
    }

    #[test]
    fn new_alarm_is_enabled_unsnoozed_and_never_fired() {
        let alarm = daily();
        assert!(alarm.enabled);
        assert_eq!(alarm.snoozed_until, None);
        assert_eq!(alarm.last_fired_at, None);
        assert_eq!(alarm.created_at, nov(&brt(), 2, 6, 0));
    }

    #[test]
    fn due_at_follows_the_schedule() {
        let tz = brt();
        assert_eq!(
            daily().due_at(nov(&tz, 2, 6, 0), &tz),
            Some(nov(&tz, 2, 7, 0))
        );
        assert_eq!(
            daily().due_at(nov(&tz, 2, 7, 0), &tz),
            Some(nov(&tz, 3, 7, 0))
        );
    }

    #[test]
    fn disabled_alarm_is_never_due() {
        let tz = brt();
        let alarm = AlarmSchedule {
            enabled: false,
            snoozed_until: Some(nov(&tz, 2, 7, 5)),
            ..daily()
        };
        assert_eq!(alarm.due_at(nov(&tz, 2, 6, 0), &tz), None);
        assert_eq!(alarm.pending(nov(&tz, 2, 8, 0), &tz), None);
    }

    #[test]
    fn snooze_takes_precedence_over_the_schedule() {
        let tz = brt();
        // Snoozed until Tuesday 08:00, past Tuesday's regular 07:00.
        let alarm = AlarmSchedule {
            snoozed_until: Some(nov(&tz, 3, 8, 0)),
            ..daily()
        };
        assert_eq!(
            alarm.due_at(nov(&tz, 2, 7, 1), &tz),
            Some(nov(&tz, 3, 8, 0))
        );
    }

    #[test]
    fn snooze_sets_the_next_ring() {
        let tz = brt();
        let alarm = daily().snooze(nov(&tz, 2, 7, 0), 5).unwrap();
        assert_eq!(alarm.snoozed_until, Some(nov(&tz, 2, 7, 5)));
        assert_eq!(
            alarm.due_at(nov(&tz, 2, 7, 0), &tz),
            Some(nov(&tz, 2, 7, 5))
        );
    }

    #[test]
    fn snooze_of_zero_minutes_is_rejected() {
        let snoozed = daily().snooze(nov(&brt(), 2, 7, 0), 0);
        assert_eq!(snoozed, Err(AlarmError::InvalidSnooze));
    }

    #[test]
    fn after_fired_disables_a_once_alarm() {
        let tz = brt();
        let fired = once().after_fired(nov(&tz, 2, 7, 0));
        assert!(!fired.enabled);
        assert_eq!(fired.last_fired_at, Some(nov(&tz, 2, 7, 0)));
        assert_eq!(fired.due_at(nov(&tz, 2, 7, 0), &tz), None);
    }

    #[test]
    fn after_fired_keeps_a_repeating_alarm_enabled() {
        let tz = brt();
        let fired = daily().after_fired(nov(&tz, 2, 7, 0));
        assert!(fired.enabled);
        assert_eq!(
            fired.due_at(nov(&tz, 2, 7, 0), &tz),
            Some(nov(&tz, 3, 7, 0))
        );
    }

    #[test]
    fn after_fired_clears_the_snooze() {
        let tz = brt();
        let snoozed = daily().snooze(nov(&tz, 2, 7, 0), 5).unwrap();
        let fired = snoozed.after_fired(nov(&tz, 2, 7, 5));
        assert_eq!(fired.snoozed_until, None);
        assert_eq!(fired.last_fired_at, Some(nov(&tz, 2, 7, 5)));
        assert_eq!(
            fired.due_at(nov(&tz, 2, 7, 5), &tz),
            Some(nov(&tz, 3, 7, 0))
        );
    }

    #[test]
    fn a_fired_once_alarm_still_rings_after_a_snooze() {
        let tz = brt();
        let fired = once().after_fired(nov(&tz, 2, 7, 0));
        let snoozed = fired.snooze(nov(&tz, 2, 7, 0), 5).unwrap();
        assert_eq!(
            snoozed.due_at(nov(&tz, 2, 7, 0), &tz),
            Some(nov(&tz, 2, 7, 5))
        );
        let done = snoozed.after_fired(nov(&tz, 2, 7, 5));
        assert!(!done.enabled);
        assert_eq!(done.due_at(nov(&tz, 2, 7, 5), &tz), None);
    }

    #[test]
    fn nothing_is_pending_before_the_first_occurrence() {
        let tz = brt();
        assert_eq!(daily().pending(nov(&tz, 2, 6, 59), &tz), None);
    }

    #[test]
    fn occurrence_noticed_within_a_minute_is_on_time() {
        let tz = brt();
        let now = nov(&tz, 2, 7, 0).plus(Duration::from_secs(30));
        assert_eq!(
            daily().pending(now, &tz),
            Some((nov(&tz, 2, 7, 0), Firing::OnTime))
        );
    }

    #[test]
    fn occurrence_up_to_thirty_minutes_ago_rings_late() {
        let tz = brt();
        let due = nov(&tz, 2, 7, 0);
        assert_eq!(
            daily().pending(nov(&tz, 2, 7, 30), &tz),
            Some((due, Firing::Late { by: mins(30) }))
        );
    }

    #[test]
    fn occurrence_over_thirty_minutes_ago_is_missed() {
        let tz = brt();
        let now = nov(&tz, 2, 7, 30).plus(Duration::from_secs(1));
        assert_eq!(
            daily().pending(now, &tz),
            Some((nov(&tz, 2, 7, 0), Firing::Missed))
        );
    }

    #[test]
    fn a_handled_occurrence_is_no_longer_pending() {
        let tz = brt();
        let fired = daily().after_fired(nov(&tz, 2, 7, 0));
        assert_eq!(fired.pending(nov(&tz, 2, 12, 0), &tz), None);
    }

    #[test]
    fn several_missed_occurrences_report_only_the_most_recent() {
        let tz = brt();
        let fired = daily().after_fired(nov(&tz, 2, 7, 0));
        assert_eq!(
            fired.pending(nov(&tz, 5, 7, 10), &tz),
            Some((nov(&tz, 5, 7, 0), Firing::Late { by: mins(10) }))
        );
        assert_eq!(
            fired.pending(nov(&tz, 5, 6, 0), &tz),
            Some((nov(&tz, 4, 7, 0), Firing::Missed))
        );
    }

    #[test]
    fn weekdays_alarm_has_nothing_pending_over_the_weekend() {
        let tz = brt();
        let alarm = AlarmSchedule::new(time(7, 0), Weekdays::WEEKDAYS, nov(&tz, 2, 6, 0))
            .after_fired(nov(&tz, 6, 7, 0));
        assert_eq!(alarm.pending(nov(&tz, 8, 12, 0), &tz), None);
        assert_eq!(
            alarm.pending(nov(&tz, 9, 9, 0), &tz),
            Some((nov(&tz, 9, 7, 0), Firing::Missed))
        );
    }

    #[test]
    fn a_long_absence_reports_the_latest_occurrence() {
        let tz = brt();
        let wed = Weekdays::single(Weekday::Wed);
        let alarm = AlarmSchedule::new(time(7, 0), wed, utc(2026, 1, 1, 0, 0));
        assert_eq!(
            alarm.pending(nov(&tz, 7, 12, 0), &tz),
            Some((nov(&tz, 4, 7, 0), Firing::Missed))
        );
    }

    #[test]
    fn a_missed_once_alarm_reports_its_own_due_time() {
        let tz = brt();
        assert_eq!(
            once().pending(nov(&tz, 6, 12, 0), &tz),
            Some((nov(&tz, 2, 7, 0), Firing::Missed))
        );
    }

    #[test]
    fn occurrences_before_creation_are_never_pending() {
        let tz = brt();
        let alarm = AlarmSchedule::new(time(7, 0), Weekdays::EVERY_DAY, nov(&tz, 2, 7, 30));
        assert_eq!(alarm.pending(nov(&tz, 2, 8, 0), &tz), None);
    }

    #[test]
    fn occurrences_before_an_edit_are_never_pending() {
        let tz = brt();
        let fired = daily().after_fired(nov(&tz, 2, 7, 0));
        // On Tuesday at 06:30 the alarm is moved to 06:00.
        let edited = AlarmSchedule {
            time: time(6, 0),
            created_at: nov(&tz, 3, 6, 30),
            ..fired
        };
        assert_eq!(edited.pending(nov(&tz, 3, 6, 40), &tz), None);
    }

    #[test]
    fn while_snoozed_only_the_snooze_is_pending() {
        let tz = brt();
        let snoozed = daily().snooze(nov(&tz, 2, 7, 0), 5).unwrap();
        assert_eq!(snoozed.pending(nov(&tz, 2, 7, 4), &tz), None);
        assert_eq!(
            snoozed.pending(nov(&tz, 2, 7, 5), &tz),
            Some((nov(&tz, 2, 7, 5), Firing::OnTime))
        );
        assert_eq!(
            snoozed.pending(nov(&tz, 2, 8, 0), &tz),
            Some((nov(&tz, 2, 7, 5), Firing::Missed))
        );
    }

    #[test]
    fn pending_follows_the_dst_gap_rule() {
        // New York: the 02:30 alarm rings at 03:00 EDT (07:00 UTC) on 2026-03-08.
        let alarm = AlarmSchedule::new(time(2, 30), Weekdays::EVERY_DAY, utc(2026, 3, 1, 0, 0))
            .after_fired(utc(2026, 3, 7, 7, 30));
        assert_eq!(
            alarm.pending(utc(2026, 3, 8, 7, 10), &New_York),
            Some((utc(2026, 3, 8, 7, 0), Firing::Late { by: mins(10) }))
        );
    }
}
