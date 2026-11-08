//! Saved countdown timers and saved pomodoros.

use std::time::Duration;

use rusqlite::{Row, params};

use super::convert::{duration_from_ms, duration_to_ms, u32_from_sql};
use super::rows::{collect, ensure_found};
use super::{Store, StoreError};
use crate::domain::{PomodoroPresetId, TaskId, TimerPresetId};

/// A saved countdown timer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerPreset {
    /// Identifier.
    pub id: TimerPresetId,
    /// Name.
    pub name: String,
    /// Duration (more than zero).
    pub duration: Duration,
    /// Linked task.
    pub task: Option<TaskId>,
}

/// Fields of a pomodoro preset to create or update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewPomodoroPreset<'a> {
    /// Name.
    pub name: &'a str,
    /// Focus phase.
    pub focus: Duration,
    /// Short break.
    pub short_break: Duration,
    /// Long break, at the end of a round.
    pub long_break: Duration,
    /// Focus phases per round (at least 1).
    pub focus_per_round: u32,
    /// Whether the next phase starts without a click.
    pub auto_start: bool,
    /// Linked task.
    pub task: Option<TaskId>,
}

/// A saved pomodoro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PomodoroPreset {
    /// Identifier.
    pub id: PomodoroPresetId,
    /// Name.
    pub name: String,
    /// Focus phase.
    pub focus: Duration,
    /// Short break.
    pub short_break: Duration,
    /// Long break, at the end of a round.
    pub long_break: Duration,
    /// Focus phases per round (at least 1).
    pub focus_per_round: u32,
    /// Whether the next phase starts without a click.
    pub auto_start: bool,
    /// Linked task.
    pub task: Option<TaskId>,
}

fn timer_preset_from_row(row: &Row<'_>) -> Result<TimerPreset, StoreError> {
    Ok(TimerPreset {
        id: TimerPresetId(row.get(0)?),
        name: row.get(1)?,
        duration: duration_from_ms(row.get(2)?, "timer preset duration")?,
        task: row.get::<_, Option<i64>>(3)?.map(TaskId),
    })
}

fn pomodoro_preset_from_row(row: &Row<'_>) -> Result<PomodoroPreset, StoreError> {
    let auto_start = match row.get::<_, i64>(6)? {
        0 => false,
        1 => true,
        other => {
            return Err(StoreError::Corrupt {
                what: "pomodoro auto_start",
                value: other.to_string(),
            });
        }
    };
    Ok(PomodoroPreset {
        id: PomodoroPresetId(row.get(0)?),
        name: row.get(1)?,
        focus: duration_from_ms(row.get(2)?, "pomodoro focus")?,
        short_break: duration_from_ms(row.get(3)?, "pomodoro short break")?,
        long_break: duration_from_ms(row.get(4)?, "pomodoro long break")?,
        focus_per_round: u32_from_sql(row.get(5)?, "pomodoro focus per round")?,
        auto_start,
        task: row.get::<_, Option<i64>>(7)?.map(TaskId),
    })
}

impl Store {
    /// Saves a countdown timer.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if `duration` is zero or the task does not exist;
    /// [`StoreError::OutOfRange`] if `duration` does not fit in milliseconds.
    pub fn create_timer_preset(
        &mut self,
        name: &str,
        duration: Duration,
        task: Option<TaskId>,
    ) -> Result<TimerPresetId, StoreError> {
        let duration_ms = duration_to_ms(duration, "timer preset duration")?;
        self.conn.execute(
            "INSERT INTO timer_presets (name, duration_ms, task_id) VALUES (?1, ?2, ?3)",
            params![name, duration_ms, task.map(|t| t.0)],
        )?;
        Ok(TimerPresetId(self.conn.last_insert_rowid()))
    }

    /// Replaces the fields of a saved countdown timer.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist; otherwise as
    /// [`Store::create_timer_preset`].
    pub fn update_timer_preset(
        &mut self,
        id: TimerPresetId,
        name: &str,
        duration: Duration,
        task: Option<TaskId>,
    ) -> Result<(), StoreError> {
        let duration_ms = duration_to_ms(duration, "timer preset duration")?;
        let changed = self.conn.execute(
            "UPDATE timer_presets SET name = ?1, duration_ms = ?2, task_id = ?3 WHERE id = ?4",
            params![name, duration_ms, task.map(|t| t.0), id.0],
        )?;
        ensure_found(changed, "timer preset", id.0)
    }

    /// Saved countdown timers, in creation order.
    ///
    /// # Errors
    /// [`StoreError::OutOfRange`] if a stored duration is negative.
    pub fn timer_presets(&self) -> Result<Vec<TimerPreset>, StoreError> {
        self.timer_presets_of(None)
    }

    /// Deletes a saved countdown timer.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn delete_timer_preset(&mut self, id: TimerPresetId) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM timer_presets WHERE id = ?1", [id.0])?;
        ensure_found(changed, "timer preset", id.0)
    }

    /// Saves a pomodoro.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if `focus_per_round` is zero or the task does not exist;
    /// [`StoreError::OutOfRange`] if a duration does not fit in milliseconds.
    pub fn create_pomodoro_preset(
        &mut self,
        preset: &NewPomodoroPreset<'_>,
    ) -> Result<PomodoroPresetId, StoreError> {
        let [focus, short_break, long_break] = pomodoro_durations(preset)?;
        self.conn.execute(
            "INSERT INTO pomodoro_presets (name, focus_ms, short_break_ms, long_break_ms,
                 focus_per_round, auto_start, task_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                preset.name,
                focus,
                short_break,
                long_break,
                preset.focus_per_round,
                preset.auto_start,
                preset.task.map(|t| t.0)
            ],
        )?;
        Ok(PomodoroPresetId(self.conn.last_insert_rowid()))
    }

    /// Replaces the fields of a saved pomodoro.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist; otherwise as
    /// [`Store::create_pomodoro_preset`].
    pub fn update_pomodoro_preset(
        &mut self,
        id: PomodoroPresetId,
        preset: &NewPomodoroPreset<'_>,
    ) -> Result<(), StoreError> {
        let [focus, short_break, long_break] = pomodoro_durations(preset)?;
        let changed = self.conn.execute(
            "UPDATE pomodoro_presets SET name = ?1, focus_ms = ?2, short_break_ms = ?3,
                 long_break_ms = ?4, focus_per_round = ?5, auto_start = ?6, task_id = ?7
             WHERE id = ?8",
            params![
                preset.name,
                focus,
                short_break,
                long_break,
                preset.focus_per_round,
                preset.auto_start,
                preset.task.map(|t| t.0),
                id.0
            ],
        )?;
        ensure_found(changed, "pomodoro preset", id.0)
    }

    /// Saved pomodoros, in creation order.
    ///
    /// # Errors
    /// [`StoreError::OutOfRange`] or [`StoreError::Corrupt`] if a stored value is invalid.
    pub fn pomodoro_presets(&self) -> Result<Vec<PomodoroPreset>, StoreError> {
        self.pomodoro_presets_of(None)
    }

    /// Deletes a saved pomodoro.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn delete_pomodoro_preset(&mut self, id: PomodoroPresetId) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM pomodoro_presets WHERE id = ?1", [id.0])?;
        ensure_found(changed, "pomodoro preset", id.0)
    }

    /// Saved countdown timers linked to `task`; all of them if `None`.
    pub(super) fn timer_presets_of(
        &self,
        task: Option<TaskId>,
    ) -> Result<Vec<TimerPreset>, StoreError> {
        let sql = "SELECT id, name, duration_ms, task_id FROM timer_presets
                   WHERE ?1 IS NULL OR task_id = ?1 ORDER BY id";
        collect(&self.conn, sql, [task.map(|t| t.0)], timer_preset_from_row)
    }

    /// Saved pomodoros linked to `task`; all of them if `None`.
    pub(super) fn pomodoro_presets_of(
        &self,
        task: Option<TaskId>,
    ) -> Result<Vec<PomodoroPreset>, StoreError> {
        let sql = "SELECT id, name, focus_ms, short_break_ms, long_break_ms, focus_per_round,
                          auto_start, task_id
                   FROM pomodoro_presets WHERE ?1 IS NULL OR task_id = ?1 ORDER BY id";
        collect(
            &self.conn,
            sql,
            [task.map(|t| t.0)],
            pomodoro_preset_from_row,
        )
    }
}

/// Focus, short break and long break of `preset`, in milliseconds.
fn pomodoro_durations(preset: &NewPomodoroPreset<'_>) -> Result<[i64; 3], StoreError> {
    Ok([
        duration_to_ms(preset.focus, "pomodoro focus")?,
        duration_to_ms(preset.short_break, "pomodoro short break")?,
        duration_to_ms(preset.long_break, "pomodoro long break")?,
    ])
}

#[cfg(test)]
mod tests;
