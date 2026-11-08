//! The alarm check of the 1 Hz tick: what came due, marked as handled right away (spec,
//! decision 4 and section 5). On time and late occurrences ring; missed ones only get a notice.
//!
//! Marking happens when the ring starts, not on Dispensar, so a restart never rings a handled
//! occurrence again. A mark that fails is retried by nobody: the occurrence is remembered for the
//! session, so it does not ring again on every tick (the store keeps reporting it).

use std::collections::HashSet;

use chrono::TimeZone;
use clocky_core::alarms::Firing;
use clocky_core::domain::AlarmId;
use clocky_core::time::Timestamp;

use super::queue::Ringing;
use super::summary;
use crate::core::{AppCore, CoreResult};

/// What one check found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Checked {
    /// Occurrences that start ringing now (on time or late), by due instant.
    pub rings: Vec<Ringing>,
    /// Notices of missed occurrences ("Alarme perdido às 07:30 · nome").
    pub missed: Vec<String>,
}

/// Occurrences already handled in this session, by alarm and due instant.
pub type Handled = HashSet<(AlarmId, Timestamp)>;

/// Asks the store what came due by `now`, marks each occurrence as fired and sorts them into
/// rings and missed notices.
///
/// # Errors
/// When the store cannot list the pending alarms (a failing mark is only logged).
pub fn check<Tz: TimeZone>(
    core: &mut AppCore,
    now: Timestamp,
    tz: &Tz,
    handled: &mut Handled,
) -> CoreResult<Checked> {
    let mut checked = Checked::default();
    for pending in core.pending_alarms(now, tz)? {
        let id = pending.alarm.id;
        if !handled.insert((id, pending.due)) {
            continue;
        }
        if let Err(e) = core.mark_alarm_fired(id, pending.due, now) {
            tracing::error!("cannot mark alarm {} as fired: {e}", id.0);
        }
        tracing::info!(
            "alarm {} due at {:?}: {:?}",
            id.0,
            pending.due,
            pending.firing
        );
        if let Some(ring) = Ringing::of(&pending) {
            checked.rings.push(ring);
        } else {
            debug_assert_eq!(pending.firing, Firing::Missed);
            let notice = summary::missed_notice(&pending.alarm.name, pending.due, now, tz);
            checked.missed.push(notice);
        }
    }
    Ok(checked)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::Utc;
    use clocky_core::alarms::{AlarmTime, Weekdays};
    use clocky_core::clock::FakeClock;
    use clocky_core::store::NewAlarm;

    use super::*;

    /// 2026-11-08 (a Sunday) at `h:m:s` UTC.
    fn at(h: u32, m: u32, s: u32) -> Timestamp {
        let t = Utc.with_ymd_and_hms(2026, 11, 8, h, m, s).unwrap();
        Timestamp::from_millis(t.timestamp_millis())
    }

    /// A core whose clock (the alarms' `created_at`) is 06:00.
    fn core() -> AppCore {
        AppCore::in_memory(Box::new(FakeClock::new(at(6, 0, 0))))
    }

    fn add(core: &mut AppCore, name: &str, h: u8, m: u8, repeat: Weekdays) -> AlarmId {
        core.create_alarm(&NewAlarm {
            name,
            time: AlarmTime::new(h, m).unwrap(),
            repeat,
            sound: None,
            message: "",
            snooze_minutes: 5,
            task: None,
        })
        .unwrap()
    }

    #[test]
    fn an_alarm_rings_once_on_time_and_is_marked() {
        let mut core = core();
        let id = add(&mut core, "standup", 7, 30, Weekdays::EVERY_DAY);
        let mut handled = Handled::new();
        assert_eq!(
            check(&mut core, at(7, 29, 59), &Utc, &mut handled).unwrap(),
            Checked::default()
        );
        let checked = check(&mut core, at(7, 30, 1), &Utc, &mut handled).unwrap();
        assert_eq!(checked.rings.len(), 1);
        assert_eq!((checked.rings[0].id, checked.rings[0].late), (id, None));
        assert_eq!(checked.missed.len(), 0);
        // Marked in the database: a fresh session (restart) does not ring it again.
        let again = check(&mut core, at(7, 30, 2), &Utc, &mut Handled::new()).unwrap();
        assert_eq!(again, Checked::default());
        assert_eq!(core.alarm(id).unwrap().last_fired_at, Some(at(7, 30, 0)));
    }

    #[test]
    fn a_late_alarm_rings_marked_late_and_an_older_one_is_missed() {
        let mut core = core();
        let late = add(&mut core, "café", 7, 0, Weekdays::ONCE);
        let missed = add(&mut core, "remédio", 6, 10, Weekdays::WEEKEND);
        let checked = check(&mut core, at(7, 12, 0), &Utc, &mut Handled::new()).unwrap();
        assert_eq!(checked.rings.len(), 1);
        assert_eq!(checked.rings[0].id, late);
        assert_eq!(checked.rings[0].late, Some(Duration::from_mins(12)));
        assert_eq!(checked.missed, ["Alarme perdido às 06:10 · remédio"]);
        // Both are handled: the once-alarm is now off, the missed one waits for next week.
        assert!(!core.alarm(late).unwrap().enabled);
        assert_eq!(
            core.alarm(missed).unwrap().last_fired_at,
            Some(at(6, 10, 0))
        );
        let again = check(&mut core, at(7, 13, 0), &Utc, &mut Handled::new()).unwrap();
        assert_eq!(again, Checked::default());
    }

    #[test]
    fn several_alarms_due_together_all_ring_in_due_order() {
        let mut core = core();
        let b = add(&mut core, "b", 7, 31, Weekdays::EVERY_DAY);
        let a = add(&mut core, "a", 7, 30, Weekdays::EVERY_DAY);
        let checked = check(&mut core, at(7, 31, 30), &Utc, &mut Handled::new()).unwrap();
        let ids: Vec<AlarmId> = checked.rings.iter().map(|r| r.id).collect();
        assert_eq!(ids, [a, b]);
    }

    #[test]
    fn an_occurrence_handled_in_this_session_never_rings_twice() {
        let mut core = core();
        let id = add(&mut core, "x", 7, 30, Weekdays::EVERY_DAY);
        let mut handled = Handled::from([(id, at(7, 30, 0))]);
        // The store still reports it (as if the mark had failed): it is skipped.
        let checked = check(&mut core, at(7, 30, 5), &Utc, &mut handled).unwrap();
        assert_eq!(checked, Checked::default());
    }

    #[test]
    fn a_snooze_rings_again_when_it_comes_due() {
        let mut core = core();
        let id = add(&mut core, "x", 7, 30, Weekdays::ONCE);
        let mut handled = Handled::new();
        check(&mut core, at(7, 30, 0), &Utc, &mut handled).unwrap();
        // The fake clock still says 06:00, so the snooze ends at 06:05: due at 07:30 already.
        let until = core.snooze_alarm(id).unwrap().unwrap();
        assert_eq!(until, at(6, 5, 0));
        let checked = check(&mut core, at(7, 30, 30), &Utc, &mut handled).unwrap();
        assert_eq!(checked.rings.len() + checked.missed.len(), 1);
    }
}
