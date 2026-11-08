//! Alarm cards of the Ativos list (SPEC 2.5): name, status, time, the 44×26 toggle, and the
//! ringing state with Dispensar. Every saved alarm has a card (the list is where they are edited
//! and switched on); disabled ones go last and are not counted in the header.

use std::time::Duration;

use chrono::TimeZone;
use clocky_core::domain::{AlarmId, TaskId};
use clocky_core::store::Alarm;
use clocky_core::time::Timestamp;

use super::queue::Queue;
use super::summary;

/// An alarm due within this long competes with the running timers for the top of the list;
/// later ones go after every timer.
pub const SOON: Duration = Duration::from_hours(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmCard {
    pub id: AlarmId,
    pub name: String,
    pub status: String,
    /// The alarm time, "07:30".
    pub time: String,
    pub enabled: bool,
    pub ringing: bool,
    pub task: Option<TaskId>,
    /// Urgency on the scale of the timer cards (`timers::cards`): lower comes first.
    pub rank: (u8, Duration),
}

impl AlarmCard {
    /// Counted in the header ("1 alarme"): enabled or ringing.
    pub fn is_active(&self) -> bool {
        self.enabled || self.ringing
    }
}

/// The card of every alarm at `now`, in the store's order (the caller sorts by `rank`).
pub fn cards<Tz: TimeZone>(
    alarms: &[Alarm],
    ringing: &Queue,
    now: Timestamp,
    tz: &Tz,
) -> Vec<AlarmCard> {
    alarms
        .iter()
        .map(|alarm| card(alarm, ringing, now, tz))
        .collect()
}

fn card<Tz: TimeZone>(alarm: &Alarm, ringing: &Queue, now: Timestamp, tz: &Tz) -> AlarmCard {
    let ring = ringing.get(alarm.id);
    let status = match ring {
        Some(r) => match r.late {
            Some(by) => format!("Tocando · {}", summary::late_label(by)),
            None => "Tocando".to_owned(),
        },
        None => summary::card_status(alarm, now, tz),
    };
    AlarmCard {
        id: alarm.id,
        name: summary::display_name(&alarm.name).to_owned(),
        status,
        time: alarm.time.to_string(),
        enabled: alarm.enabled,
        ringing: ring.is_some(),
        task: alarm.task,
        rank: rank(alarm, ring.is_some(), now, tz),
    }
}

/// Ringing first (0, as ringing countdowns); alarms due within [`SOON`] among the running timers
/// (2, by time left); later ones after every timer (6); disabled ones last (7).
fn rank<Tz: TimeZone>(alarm: &Alarm, ringing: bool, now: Timestamp, tz: &Tz) -> (u8, Duration) {
    if ringing {
        return (0, Duration::ZERO);
    }
    match alarm.schedule().due_at(now, tz) {
        Some(due) => {
            let left = due.since(now);
            (if left <= SOON { 2 } else { 6 }, left)
        }
        None => (7, Duration::ZERO),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use clocky_core::alarms::{AlarmTime, Firing, Weekdays};
    use clocky_core::store::PendingAlarm;

    use super::*;
    use crate::alarms::queue::Ringing;

    fn at(h: u32, m: u32) -> Timestamp {
        let t = Utc.with_ymd_and_hms(2026, 11, 8, h, m, 0).unwrap();
        Timestamp::from_millis(t.timestamp_millis())
    }

    fn alarm(id: i64, h: u8, m: u8, enabled: bool) -> Alarm {
        Alarm {
            id: AlarmId(id),
            name: format!("a{id}"),
            time: AlarmTime::new(h, m).unwrap(),
            repeat: Weekdays::EVERY_DAY,
            sound: None,
            message: String::new(),
            snooze_minutes: 5,
            task: None,
            enabled,
            snoozed_until: None,
            last_fired_at: None,
            created_at: at(0, 0),
        }
    }

    fn ringing(alarm: &Alarm, firing: Firing) -> Ringing {
        Ringing::of(&PendingAlarm {
            alarm: alarm.clone(),
            due: at(7, 0),
            firing,
        })
        .unwrap()
    }

    #[test]
    fn soon_alarms_rank_with_timers_later_ones_after_and_off_ones_last() {
        let alarms = [
            alarm(1, 12, 30, true),
            alarm(2, 10, 30, true),
            alarm(3, 10, 0, false),
        ];
        let cards = cards(&alarms, &Queue::default(), at(10, 0), &Utc);
        assert_eq!(cards[0].rank, (6, Duration::from_mins(150)));
        assert_eq!(cards[1].rank, (2, Duration::from_mins(30)));
        assert_eq!(cards[2].rank, (7, Duration::ZERO));
        assert_eq!(cards[1].status, "todos os dias · hoje");
        assert_eq!(cards[1].time, "10:30");
        assert!(cards[1].is_active() && !cards[2].is_active());
    }

    #[test]
    fn a_ringing_alarm_says_so_and_comes_first_even_if_disabled() {
        let once = Alarm {
            enabled: false,
            ..alarm(1, 7, 0, true)
        };
        let mut queue = Queue::default();
        queue.push(ringing(
            &once,
            Firing::Late {
                by: Duration::from_mins(12),
            },
        ));
        let late = cards(std::slice::from_ref(&once), &queue, at(7, 12), &Utc);
        assert!(late[0].ringing && late[0].is_active());
        assert_eq!(late[0].status, "Tocando · atrasado 12 min");
        assert_eq!(late[0].rank, (0, Duration::ZERO));
        let mut on_time = Queue::default();
        on_time.push(ringing(&once, Firing::OnTime));
        assert_eq!(
            cards(&[once], &on_time, at(7, 0), &Utc)[0].status,
            "Tocando"
        );
    }
}
