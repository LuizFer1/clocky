//! Tests for the alarm schedule.

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
fn snooze_does_not_re_enable_a_disabled_repeating_alarm() {
    let tz = brt();
    let disabled = AlarmSchedule {
        enabled: false,
        ..daily()
    };
    let snoozed = disabled.snooze(nov(&tz, 2, 7, 0), 5).unwrap();
    assert!(!snoozed.enabled);
    assert_eq!(snoozed.due_at(nov(&tz, 2, 7, 0), &tz), None);
    assert_eq!(snoozed.pending(nov(&tz, 2, 7, 5), &tz), None);
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

#[test]
fn an_alarm_still_rings_after_the_clock_moves_back_past_its_last_fire() {
    let tz = brt();
    // Fired on Tuesday the 3rd, then the clock was corrected back to Monday the 2nd.
    let fired = daily().after_fired(nov(&tz, 3, 7, 0));
    assert_eq!(
        fired.due_at(nov(&tz, 2, 6, 0), &tz),
        Some(nov(&tz, 2, 7, 0))
    );
    assert_eq!(
        fired.pending(nov(&tz, 2, 7, 0), &tz),
        Some((nov(&tz, 2, 7, 0), Firing::OnTime))
    );
    let rang_again = fired.after_fired(nov(&tz, 2, 7, 0));
    assert_eq!(
        rang_again.pending(nov(&tz, 3, 7, 0), &tz),
        Some((nov(&tz, 3, 7, 0), Firing::OnTime))
    );
}

#[test]
fn an_alarm_still_rings_after_the_clock_moves_back_past_its_creation() {
    let tz = brt();
    let created_later = AlarmSchedule::new(time(7, 0), Weekdays::EVERY_DAY, nov(&tz, 3, 6, 0));
    assert_eq!(
        created_later.pending(nov(&tz, 2, 7, 0), &tz),
        Some((nov(&tz, 2, 7, 0), Firing::OnTime))
    );
}

#[test]
fn a_clock_moved_back_never_reports_handled_history_as_missed() {
    let tz = brt();
    // Fired on the 3rd at 07:00, then corrected back 5 minutes: the 2nd's 07:00 was handled.
    let fired = daily().after_fired(nov(&tz, 3, 7, 0));
    assert_eq!(fired.pending(nov(&tz, 3, 6, 55), &tz), None);
    let created_later = AlarmSchedule::new(time(7, 0), Weekdays::EVERY_DAY, nov(&tz, 3, 6, 0));
    assert_eq!(created_later.pending(nov(&tz, 2, 8, 0), &tz), None);
}

#[test]
fn a_small_backward_correction_can_ring_an_occurrence_twice() {
    let tz = brt();
    // 07:00 was snoozed and rang again at 07:05; then the clock went back 3 minutes.
    let snoozed = daily().snooze(nov(&tz, 2, 7, 0), 5).unwrap();
    let fired = snoozed.after_fired(nov(&tz, 2, 7, 5));
    assert_eq!(
        fired.pending(nov(&tz, 2, 7, 2), &tz),
        Some((nov(&tz, 2, 7, 0), Firing::Late { by: mins(2) }))
    );
}
