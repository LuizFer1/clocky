//! Alarms (migration 3): create, read, update and delete, with their scheduling state.
//!
//! Editing or enabling an alarm again re-arms it: `created_at` becomes `now`, so occurrences
//! from before the change are never reported as missed (see [`AlarmSchedule::created_at`]).

use rusqlite::{Connection, Row, params};

use super::convert::u32_from_sql;
use super::rows::{collect, ensure_found, first};
use super::{Store, StoreError};
use crate::alarms::{AlarmSchedule, AlarmTime, Weekdays};
use crate::domain::{AlarmId, TaskId};
use crate::time::Timestamp;

mod runtime;

pub use runtime::PendingAlarm;

/// Fields of an alarm to create or update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewAlarm<'a> {
    /// Name.
    pub name: &'a str,
    /// Local time at which it rings.
    pub time: AlarmTime,
    /// Days on which it repeats; [`Weekdays::ONCE`] rings once.
    pub repeat: Weekdays,
    /// Custom sound file; `None` = the default sound.
    pub sound: Option<&'a str>,
    /// Text shown when it rings (may be empty).
    pub message: &'a str,
    /// Snooze length in minutes (at least 1).
    pub snooze_minutes: u32,
    /// Linked task.
    pub task: Option<TaskId>,
}

/// A saved alarm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alarm {
    /// Identifier.
    pub id: AlarmId,
    /// Name.
    pub name: String,
    /// Local time at which it rings.
    pub time: AlarmTime,
    /// Days on which it repeats; [`Weekdays::ONCE`] rings once.
    pub repeat: Weekdays,
    /// Custom sound file; `None` = the default sound.
    pub sound: Option<String>,
    /// Text shown when it rings (may be empty).
    pub message: String,
    /// Snooze length in minutes (at least 1).
    pub snooze_minutes: u32,
    /// Linked task.
    pub task: Option<TaskId>,
    /// Whether it is active.
    pub enabled: bool,
    /// While snoozed, the instant at which it rings again.
    pub snoozed_until: Option<Timestamp>,
    /// The last occurrence that was handled (rang or was reported as missed).
    pub last_fired_at: Option<Timestamp>,
    /// When it was created, edited or last enabled again.
    pub created_at: Timestamp,
}

impl Alarm {
    /// The scheduling state of the alarm, for the rules in [`crate::alarms`].
    #[must_use]
    pub fn schedule(&self) -> AlarmSchedule {
        AlarmSchedule {
            time: self.time,
            repeat: self.repeat,
            enabled: self.enabled,
            snoozed_until: self.snoozed_until,
            last_fired_at: self.last_fired_at,
            created_at: self.created_at,
        }
    }
}

/// Columns read by [`alarm_from_row`].
const ALARM_COLUMNS: &str = "id, name, time_local, repeat_days, sound_path, message, \
     snooze_minutes, task_id, enabled, snoozed_until_ms, last_fired_at_ms, created_at_ms";

fn alarm_from_row(row: &Row<'_>) -> Result<Alarm, StoreError> {
    Ok(Alarm {
        id: AlarmId(row.get(0)?),
        name: row.get(1)?,
        time: time_from_sql(&row.get::<_, String>(2)?)?,
        repeat: weekdays_from_sql(row.get(3)?)?,
        sound: row.get(4)?,
        message: row.get(5)?,
        snooze_minutes: snooze_from_sql(row.get(6)?)?,
        task: row.get::<_, Option<i64>>(7)?.map(TaskId),
        enabled: flag_from_sql(row.get(8)?)?,
        snoozed_until: row
            .get::<_, Option<i64>>(9)?
            .map(|ms| instant_from_ms(ms, "alarm snoozed until"))
            .transpose()?,
        last_fired_at: row
            .get::<_, Option<i64>>(10)?
            .map(|ms| instant_from_ms(ms, "alarm last fired at"))
            .transpose()?,
        created_at: instant_from_ms(row.get(11)?, "alarm created at")?,
    })
}

/// The `time_local` column (`HH:MM`).
fn time_from_sql(text: &str) -> Result<AlarmTime, StoreError> {
    text.parse().map_err(|_| StoreError::Corrupt {
        what: "alarm time",
        value: text.to_owned(),
    })
}

/// The `repeat_days` column (bitmask from 0 to 127).
fn weekdays_from_sql(bits: i64) -> Result<Weekdays, StoreError> {
    u8::try_from(bits)
        .ok()
        .and_then(|bits| Weekdays::try_from(bits).ok())
        .ok_or_else(|| StoreError::Corrupt {
            what: "alarm repeat days",
            value: bits.to_string(),
        })
}

/// The `snooze_minutes` column (at least 1).
fn snooze_from_sql(minutes: i64) -> Result<u32, StoreError> {
    match u32_from_sql(minutes, "alarm snooze minutes")? {
        0 => Err(StoreError::Corrupt {
            what: "alarm snooze minutes",
            value: "0".to_owned(),
        }),
        minutes => Ok(minutes),
    }
}

/// The `enabled` column (0 or 1).
fn flag_from_sql(value: i64) -> Result<bool, StoreError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(StoreError::Corrupt {
            what: "alarm enabled",
            value: other.to_string(),
        }),
    }
}

/// An instant read from the database; negative values are an error (no alarm is that old).
fn instant_from_ms(millis: i64, what: &'static str) -> Result<Timestamp, StoreError> {
    if millis < 0 {
        return Err(StoreError::OutOfRange(what));
    }
    Ok(Timestamp::from_millis(millis))
}

/// An instant to write; negative values are an error, as when reading them back.
fn instant_to_sql(instant: Timestamp, what: &'static str) -> Result<i64, StoreError> {
    let millis = instant.as_millis();
    if millis < 0 {
        return Err(StoreError::OutOfRange(what));
    }
    Ok(millis)
}

/// The alarm with this identifier, read through `conn` (a store or a transaction).
fn alarm_in(conn: &Connection, id: AlarmId) -> Result<Alarm, StoreError> {
    let sql = format!("SELECT {ALARM_COLUMNS} FROM alarms WHERE id = ?1");
    first(collect(conn, &sql, [id.0], alarm_from_row)?, "alarm", id.0)
}

impl Store {
    /// Saves a new alarm, enabled and armed from `now`.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if `snooze_minutes` is zero or the task does not exist.
    pub fn create_alarm(
        &mut self,
        alarm: &NewAlarm<'_>,
        now: Timestamp,
    ) -> Result<AlarmId, StoreError> {
        let created_at = instant_to_sql(now, "alarm created at")?;
        self.conn.execute(
            "INSERT INTO alarms (name, time_local, repeat_days, sound_path, message,
                 snooze_minutes, task_id, enabled, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
            params![
                alarm.name,
                alarm.time.to_string(),
                alarm.repeat.bits(),
                alarm.sound,
                alarm.message,
                alarm.snooze_minutes,
                alarm.task.map(|t| t.0),
                created_at
            ],
        )?;
        Ok(AlarmId(self.conn.last_insert_rowid()))
    }

    /// The alarm with this identifier.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist; [`StoreError::Corrupt`] or
    /// [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn alarm(&self, id: AlarmId) -> Result<Alarm, StoreError> {
        alarm_in(&self.conn, id)
    }

    /// Every alarm, by time of day and then by creation.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn alarms(&self) -> Result<Vec<Alarm>, StoreError> {
        self.alarms_of(None)
    }

    /// Replaces the fields of an alarm and re-arms it: `created_at` becomes `now` and the
    /// snooze is cleared. Whether it is enabled and when it last fired are kept, except for a
    /// snoozed alarm that stays a once-alarm: it already fired (the snooze enabled it again), so
    /// with the snooze gone it is disabled instead of ringing at its time the next day. Made
    /// repeating, it stays enabled.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist; otherwise as [`Store::create_alarm`].
    pub fn update_alarm(
        &mut self,
        id: AlarmId,
        alarm: &NewAlarm<'_>,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let created_at = instant_to_sql(now, "alarm created at")?;
        let changed = self.conn.execute(
            "UPDATE alarms SET name = ?2, time_local = ?3, repeat_days = ?4, sound_path = ?5,
                 message = ?6, snooze_minutes = ?7, task_id = ?8, created_at_ms = ?9,
                 snoozed_until_ms = NULL,
                 enabled = CASE WHEN ?4 = 0 AND snoozed_until_ms IS NOT NULL
                                THEN 0 ELSE enabled END
             WHERE id = ?1",
            params![
                id.0,
                alarm.name,
                alarm.time.to_string(),
                alarm.repeat.bits(),
                alarm.sound,
                alarm.message,
                alarm.snooze_minutes,
                alarm.task.map(|t| t.0),
                created_at
            ],
        )?;
        ensure_found(changed, "alarm", id.0)
    }

    /// Deletes an alarm.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn delete_alarm(&mut self, id: AlarmId) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM alarms WHERE id = ?1", [id.0])?;
        ensure_found(changed, "alarm", id.0)
    }

    /// Turns an alarm on or off.
    ///
    /// Turning on a disabled alarm re-arms it: `created_at` becomes `now` and any snooze left
    /// behind is cleared. Turning on an enabled one changes nothing, so a pending occurrence is
    /// not lost. Turning it off clears the snooze.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn set_alarm_enabled(
        &mut self,
        id: AlarmId,
        enabled: bool,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let changed = if enabled {
            self.conn.execute(
                "UPDATE alarms SET created_at_ms = CASE WHEN enabled THEN created_at_ms ELSE ?2 END,
                     snoozed_until_ms = CASE WHEN enabled THEN snoozed_until_ms ELSE NULL END,
                     enabled = 1
                 WHERE id = ?1",
                params![id.0, instant_to_sql(now, "alarm created at")?],
            )?
        } else {
            self.conn.execute(
                "UPDATE alarms SET enabled = 0, snoozed_until_ms = NULL WHERE id = ?1",
                [id.0],
            )?
        };
        ensure_found(changed, "alarm", id.0)
    }

    /// Links an alarm to `task` (`None` = unlinks it).
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the alarm does not exist; [`StoreError::Sqlite`] if the task
    /// does not exist.
    pub fn set_alarm_task(&mut self, id: AlarmId, task: Option<TaskId>) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE alarms SET task_id = ?2 WHERE id = ?1",
            params![id.0, task.map(|t| t.0)],
        )?;
        ensure_found(changed, "alarm", id.0)
    }

    /// Alarms linked to `task`, by time of day; all of them if `None`.
    pub(super) fn alarms_of(&self, task: Option<TaskId>) -> Result<Vec<Alarm>, StoreError> {
        let sql = format!(
            "SELECT {ALARM_COLUMNS} FROM alarms
             WHERE ?1 IS NULL OR task_id = ?1 ORDER BY time_local, id"
        );
        collect(&self.conn, &sql, [task.map(|t| t.0)], alarm_from_row)
    }
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
