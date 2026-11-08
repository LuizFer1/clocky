//! Alarms ringing right now, in the order they started. One overlay shows the first one with
//! "1 de N"; Dispensar or Soneca moves to the next (decision logged in `docs/duvidas.md`).

use std::time::Duration;

use clocky_core::alarms::{AlarmTime, Firing};
use clocky_core::domain::{AlarmId, TaskId};
use clocky_core::store::{Alarm, PendingAlarm};
use clocky_core::time::Timestamp;

/// One ringing alarm, copied from the database when it started ringing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ringing {
    pub id: AlarmId,
    /// The occurrence that rings (already marked as fired).
    pub due: Timestamp,
    pub name: String,
    pub message: String,
    pub time: AlarmTime,
    pub task: Option<TaskId>,
    /// Custom sound file; `None` = the built-in chime.
    pub sound: Option<String>,
    pub snooze_minutes: u32,
    /// How late it rings (`None` = on time).
    pub late: Option<Duration>,
}

impl Ringing {
    /// The ring of a pending occurrence; `None` for a missed one (it only gets a notice).
    pub fn of(pending: &PendingAlarm) -> Option<Self> {
        let late = match pending.firing {
            Firing::OnTime => None,
            Firing::Late { by } => Some(by),
            Firing::Missed => return None,
        };
        let alarm = &pending.alarm;
        Some(Self {
            id: alarm.id,
            due: pending.due,
            name: alarm.name.clone(),
            message: alarm.message.clone(),
            time: alarm.time,
            task: alarm.task,
            sound: alarm.sound.clone(),
            snooze_minutes: alarm.snooze_minutes,
            late,
        })
    }
}

#[derive(Debug, Default)]
pub struct Queue {
    items: Vec<Ringing>,
}

impl Queue {
    /// Adds a ring. An alarm already ringing (its snooze came due while it still rang) is
    /// replaced in place, so it shows once. Returns whether it became the current one.
    pub fn push(&mut self, ringing: Ringing) -> bool {
        if let Some(index) = self.items.iter().position(|r| r.id == ringing.id) {
            self.items[index] = ringing;
            return index == 0;
        }
        self.items.push(ringing);
        self.items.len() == 1
    }

    /// The ring the overlay shows.
    pub fn current(&self) -> Option<&Ringing> {
        self.items.first()
    }

    /// Removes the ring of alarm `id`, if it rings.
    pub fn remove(&mut self, id: AlarmId) -> Option<Ringing> {
        let index = self.items.iter().position(|r| r.id == id)?;
        Some(self.items.remove(index))
    }

    /// Copies the fields of a saved `alarm` into its ring (the occurrence and lateness stay).
    /// `None` when it is not ringing; otherwise whether its sound changed.
    pub fn refresh(&mut self, alarm: &Alarm) -> Option<bool> {
        let ring = self.items.iter_mut().find(|r| r.id == alarm.id)?;
        let sound_changed = ring.sound != alarm.sound;
        ring.name.clone_from(&alarm.name);
        ring.message.clone_from(&alarm.message);
        ring.time = alarm.time;
        ring.task = alarm.task;
        ring.sound.clone_from(&alarm.sound);
        ring.snooze_minutes = alarm.snooze_minutes;
        Some(sound_changed)
    }

    pub fn get(&self, id: AlarmId) -> Option<&Ringing> {
        self.items.iter().find(|r| r.id == id)
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use clocky_core::alarms::Weekdays;

    use super::*;

    fn pending(id: i64, firing: Firing) -> PendingAlarm {
        PendingAlarm {
            alarm: Alarm {
                id: AlarmId(id),
                name: format!("a{id}"),
                time: AlarmTime::new(7, 30).unwrap(),
                repeat: Weekdays::ONCE,
                sound: None,
                message: "Hora!".into(),
                snooze_minutes: 5,
                task: None,
                enabled: true,
                snoozed_until: None,
                last_fired_at: None,
                created_at: Timestamp::from_millis(0),
            },
            due: Timestamp::from_millis(id * 1000),
            firing,
        }
    }

    fn ring(id: i64) -> Ringing {
        Ringing::of(&pending(id, Firing::OnTime)).unwrap()
    }

    #[test]
    fn missed_occurrences_do_not_ring() {
        assert!(Ringing::of(&pending(1, Firing::Missed)).is_none());
        let late = Ringing::of(&pending(
            1,
            Firing::Late {
                by: Duration::from_mins(5),
            },
        ))
        .unwrap();
        assert_eq!(late.late, Some(Duration::from_mins(5)));
        assert_eq!(ring(1).late, None);
        assert_eq!(ring(1).message, "Hora!");
    }

    #[test]
    fn rings_queue_in_order_and_the_first_is_current() {
        let mut q = Queue::default();
        assert!(q.push(ring(1)));
        assert!(!q.push(ring(2)));
        assert_eq!(q.count(), 2);
        assert_eq!(q.current().unwrap().id, AlarmId(1));
        assert_eq!(q.remove(AlarmId(1)).unwrap().id, AlarmId(1));
        assert_eq!(q.current().unwrap().id, AlarmId(2));
        assert!(q.remove(AlarmId(1)).is_none());
        q.remove(AlarmId(2));
        assert!(q.count() == 0 && q.current().is_none());
    }

    #[test]
    fn an_alarm_that_rings_again_is_replaced_not_duplicated() {
        let mut q = Queue::default();
        q.push(ring(1));
        q.push(ring(2));
        let again = Ringing {
            due: Timestamp::from_millis(9_000),
            ..ring(2)
        };
        assert!(!q.push(again));
        assert_eq!(q.count(), 2);
        assert_eq!(
            q.get(AlarmId(2)).unwrap().due,
            Timestamp::from_millis(9_000)
        );
        assert!(q.get(AlarmId(3)).is_none());
        let first_again = Ringing {
            due: Timestamp::from_millis(8_000),
            ..ring(1)
        };
        assert!(q.push(first_again));
        assert_eq!(q.count(), 2);
    }

    #[test]
    fn a_saved_alarm_refreshes_its_ring_and_keeps_the_occurrence() {
        let mut q = Queue::default();
        q.push(ring(1));
        let mut saved = pending(1, Firing::OnTime).alarm;
        saved.name = "novo".into();
        saved.snooze_minutes = 10;
        assert_eq!(q.refresh(&saved), Some(false));
        saved.sound = Some("C:/galo.mp3".into());
        assert_eq!(q.refresh(&saved), Some(true));
        let r = q.get(AlarmId(1)).unwrap();
        assert_eq!((r.name.as_str(), r.snooze_minutes), ("novo", 10));
        assert_eq!(r.due, Timestamp::from_millis(1000));
        saved.id = AlarmId(9);
        assert_eq!(q.refresh(&saved), None);
    }
}
