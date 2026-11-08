//! Alarm runtime: what rings now, what rings next, and handling a ring.

use chrono::TimeZone;
use rusqlite::params;

use super::{Alarm, Store, StoreError, alarm_in};
use crate::alarms::{AlarmSchedule, Firing};
use crate::domain::AlarmId;
use crate::time::Timestamp;

impl Store {
    /// Every alarm with an occurrence that came due by `now` and was not handled yet, with how
    /// to present it (see [`AlarmSchedule::pending`]), by due instant and then by identifier.
    ///
    /// `tz` is the local time zone (the app passes `&chrono::Local`). Handle each one with
    /// [`Store::mark_alarm_fired`], missed ones too, or it stays pending.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn pending_alarms<Tz: TimeZone>(
        &self,
        now: Timestamp,
        tz: &Tz,
    ) -> Result<Vec<(AlarmId, Timestamp, Firing)>, StoreError> {
        let mut pending: Vec<_> = self
            .alarms()?
            .iter()
            .filter_map(|alarm| {
                let (due, firing) = alarm.schedule().pending(now, tz)?;
                Some((alarm.id, due, firing))
            })
            .collect();
        pending.sort_by_key(|&(id, due, _)| (due, id));
        Ok(pending)
    }

    /// The enabled alarm that rings next after `now` and when (the "Active" view); a snooze
    /// counts as its next ring. `None` if no alarm is enabled.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn next_alarm<Tz: TimeZone>(
        &self,
        now: Timestamp,
        tz: &Tz,
    ) -> Result<Option<(AlarmId, Timestamp)>, StoreError> {
        Ok(self
            .alarms()?
            .iter()
            .filter_map(|alarm| Some((alarm.id, alarm.schedule().due_at(now, tz)?)))
            .min_by_key(|&(id, due)| (due, id)))
    }

    /// Records that the occurrence due at `at` was handled (it rang or was reported as
    /// missed): clears the snooze, sets `last_fired_at` and disables a once-alarm.
    ///
    /// `at` is the due instant returned by [`Store::pending_alarms`], not the current time.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the alarm does not exist; [`StoreError::Corrupt`] or
    /// [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn mark_alarm_fired(&mut self, id: AlarmId, at: Timestamp) -> Result<(), StoreError> {
        self.change_schedule(id, |alarm| Ok(alarm.schedule().after_fired(at)))?;
        Ok(())
    }

    /// Snoozes the ringing alarm at `now` for its `snooze_minutes`, and returns when it rings
    /// again. A fired once-alarm is enabled again until the snooze rings.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the alarm does not exist; [`StoreError::Corrupt`] or
    /// [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn snooze_alarm(&mut self, id: AlarmId, now: Timestamp) -> Result<Timestamp, StoreError> {
        let schedule = self.change_schedule(id, |alarm| {
            alarm
                .schedule()
                .snooze(now, alarm.snooze_minutes)
                .map_err(|error| StoreError::Corrupt {
                    what: "alarm snooze minutes",
                    value: error.to_string(),
                })
        })?;
        schedule.snoozed_until.ok_or(StoreError::Corrupt {
            what: "alarm snooze",
            value: "not set after snoozing".to_owned(),
        })
    }

    /// Reads an alarm, applies `change` to its schedule and writes the result, in one
    /// transaction.
    fn change_schedule(
        &mut self,
        id: AlarmId,
        change: impl FnOnce(&Alarm) -> Result<AlarmSchedule, StoreError>,
    ) -> Result<AlarmSchedule, StoreError> {
        let tx = self.conn.transaction()?;
        let schedule = change(&alarm_in(&tx, id)?)?;
        tx.execute(
            "UPDATE alarms SET enabled = ?2, snoozed_until_ms = ?3, last_fired_at_ms = ?4
             WHERE id = ?1",
            params![
                id.0,
                schedule.enabled,
                schedule.snoozed_until.map(Timestamp::as_millis),
                schedule.last_fired_at.map(Timestamp::as_millis)
            ],
        )?;
        tx.commit()?;
        Ok(schedule)
    }
}

#[cfg(test)]
mod tests;
