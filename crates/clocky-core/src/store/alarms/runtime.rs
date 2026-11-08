//! Alarm runtime: what rings now, what rings next, and handling a ring.

use chrono::TimeZone;
use rusqlite::params;

use super::{Alarm, Store, StoreError, alarm_in, instant_to_sql};
use crate::alarms::{AlarmSchedule, Firing};
use crate::domain::AlarmId;
use crate::time::Timestamp;

/// An alarm occurrence that came due and was not handled yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAlarm {
    /// The alarm, as stored (name, message, sound... for the overlay).
    pub alarm: Alarm,
    /// When the occurrence came due: pass it to [`Store::mark_alarm_fired`].
    pub due: Timestamp,
    /// How to present it: on time, late or missed.
    pub firing: Firing,
}

impl Store {
    /// Every alarm with an occurrence that came due by `now` and was not handled yet, with the
    /// alarm itself and how to present it (see [`AlarmSchedule::pending`]), by due instant and
    /// then by identifier, so the tick needs no other lookup.
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
    ) -> Result<Vec<PendingAlarm>, StoreError> {
        let mut pending: Vec<_> = self
            .alarms()?
            .into_iter()
            .filter_map(|alarm| {
                let (due, firing) = alarm.schedule().pending(now, tz)?;
                Some(PendingAlarm { alarm, due, firing })
            })
            .collect();
        pending.sort_by_key(|p| (p.due, p.alarm.id));
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
    /// `at` is the due instant returned by [`Store::pending_alarms`] and `now` the instant
    /// passed to it: call it in the same tick, before any [`Store::snooze_alarm`].
    ///
    /// A stale mark is ignored, so it never drops a snooze or moves `last_fired_at` back (which
    /// would ring a later occurrence twice): when the alarm is snoozed until after `at`, or
    /// already handled an occurrence after `at` that is not in the future. A `last_fired_at`
    /// later than `now` means the clock moved back; [`AlarmSchedule::pending`] ignores it and
    /// reports `at` again, so that mark is written, or the alarm would ring on every tick.
    ///
    /// Marking the snooze ring itself (`at` equals `snoozed_until`) is never skipped, even when
    /// an earlier fire is recorded after it (the clock moved back before the snooze); the later
    /// `last_fired_at` is kept.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the alarm does not exist; [`StoreError::Corrupt`] or
    /// [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn mark_alarm_fired(
        &mut self,
        id: AlarmId,
        at: Timestamp,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        self.change_schedule(id, |alarm| {
            let fired_later = alarm
                .last_fired_at
                .filter(|fired| *fired > at && *fired <= now);
            if alarm.snoozed_until == Some(at) {
                // The snooze ring itself: always handled, keeping a later known fire.
                let handled = alarm.schedule().after_fired(at);
                let last_fired_at = fired_later.or(handled.last_fired_at);
                return Ok(Some(AlarmSchedule {
                    last_fired_at,
                    ..handled
                }));
            }
            let snoozed_later = alarm.snoozed_until.is_some_and(|until| until > at);
            let stale = snoozed_later || fired_later.is_some();
            Ok((!stale).then(|| alarm.schedule().after_fired(at)))
        })?;
        Ok(())
    }

    /// Snoozes the ringing alarm at `now` for its `snooze_minutes`, and returns when it rings
    /// again.
    ///
    /// A fired once-alarm is enabled again until the snooze rings. A repeating alarm the user
    /// disabled while it rang is left alone and `None` is returned: the overlay just closes,
    /// and no snooze is left behind to ring when the alarm is enabled again.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the alarm does not exist; [`StoreError::Corrupt`] or
    /// [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn snooze_alarm(
        &mut self,
        id: AlarmId,
        now: Timestamp,
    ) -> Result<Option<Timestamp>, StoreError> {
        let schedule = self.change_schedule(id, |alarm| {
            if !alarm.enabled && !alarm.repeat.is_once() {
                return Ok(None);
            }
            let snoozed = alarm.schedule().snooze(now, alarm.snooze_minutes);
            snoozed.map(Some).map_err(|error| StoreError::Corrupt {
                what: "alarm snooze minutes",
                value: error.to_string(),
            })
        })?;
        Ok(schedule.and_then(|schedule| schedule.snoozed_until))
    }

    /// Reads an alarm, applies `change` to its schedule and writes the result, in one
    /// transaction. Nothing is written if `change` returns `None`.
    fn change_schedule(
        &mut self,
        id: AlarmId,
        change: impl FnOnce(&Alarm) -> Result<Option<AlarmSchedule>, StoreError>,
    ) -> Result<Option<AlarmSchedule>, StoreError> {
        let tx = self.conn.transaction()?;
        let Some(schedule) = change(&alarm_in(&tx, id)?)? else {
            return Ok(None);
        };
        let snoozed_until = schedule
            .snoozed_until
            .map(|until| instant_to_sql(until, "alarm snoozed until"))
            .transpose()?;
        let last_fired_at = schedule
            .last_fired_at
            .map(|fired| instant_to_sql(fired, "alarm last fired at"))
            .transpose()?;
        tx.execute(
            "UPDATE alarms SET enabled = ?2, snoozed_until_ms = ?3, last_fired_at_ms = ?4
             WHERE id = ?1",
            params![id.0, schedule.enabled, snoozed_until, last_fired_at],
        )?;
        tx.commit()?;
        Ok(Some(schedule))
    }
}

#[cfg(test)]
mod tests;
