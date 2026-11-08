//! Active timers: saved on every state change so they survive closing the app.
//!
//! Each timer is one `active_timers` row plus its run in `active_timer_segments` (the open
//! segment has `end_ms` NULL). Rows are read back through the engine's `from_parts`, so an
//! inconsistent row is an `Err`, never a panic.

mod load;

use rusqlite::{Transaction, params};

use super::convert::{duration_to_ms, timer_id_from_sql, timer_id_to_sql};
use super::rows::collect;
use super::{Store, StoreError};
use crate::domain::{PresetRef, TaskId, TimerId};
use crate::engine::pomodoro::{Phase, Pomodoro};
use crate::engine::run::Run;
use crate::engine::session::TimerKind;
use crate::engine::timers::{ActiveTimer, Timer};

/// An active timer linked to a task ("Linked to").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveTimerLink {
    /// The timer.
    pub timer: TimerId,
    /// Its kind.
    pub kind: TimerKind,
}

/// Columns of an `active_timers` row, as written.
#[derive(Debug, Default)]
struct Columns<'a> {
    kind: &'static str,
    timer_preset: Option<i64>,
    pomodoro_preset: Option<i64>,
    target_ms: Option<i64>,
    focus_ms: Option<i64>,
    short_break_ms: Option<i64>,
    long_break_ms: Option<i64>,
    focus_per_round: Option<u32>,
    auto_start: Option<bool>,
    phase: Option<&'static str>,
    focus_index: Option<u32>,
    round: Option<u32>,
    phase_started: Option<bool>,
    run: Option<&'a Run>,
}

impl<'a> Columns<'a> {
    fn of(active: &'a ActiveTimer) -> Result<Self, StoreError> {
        let (timer_preset, pomodoro_preset) = preset_to_sql(active)?;
        Ok(match &active.timer {
            Timer::Stopwatch(sw) => Self {
                kind: "stopwatch",
                run: Some(sw.run()),
                ..Self::default()
            },
            Timer::Countdown(cd) => Self {
                kind: "countdown",
                timer_preset,
                target_ms: Some(duration_to_ms(cd.target(), "countdown target")?),
                run: Some(cd.run()),
                ..Self::default()
            },
            Timer::Pomodoro(p) => Self::of_pomodoro(p, pomodoro_preset)?,
        })
    }

    fn of_pomodoro(p: &'a Pomodoro, preset: Option<i64>) -> Result<Self, StoreError> {
        let config = p.config();
        let phase_ms = |phase| duration_to_ms(config.duration(phase), "pomodoro phase");
        Ok(Self {
            kind: "pomodoro",
            pomodoro_preset: preset,
            focus_ms: Some(phase_ms(Phase::Focus)?),
            short_break_ms: Some(phase_ms(Phase::ShortBreak)?),
            long_break_ms: Some(phase_ms(Phase::LongBreak)?),
            focus_per_round: Some(config.focus_per_round()),
            auto_start: Some(config.auto_start()),
            phase: Some(phase_to_sql(p.phase())),
            focus_index: Some(p.focus_index()),
            round: Some(p.round()),
            phase_started: Some(!p.is_waiting()),
            run: p.run(),
            ..Self::default()
        })
    }
}

/// The `(timer_preset_id, pomodoro_preset_id)` columns.
fn preset_to_sql(active: &ActiveTimer) -> Result<(Option<i64>, Option<i64>), StoreError> {
    match (&active.timer, active.preset) {
        (_, None) => Ok((None, None)),
        (Timer::Countdown(_), Some(PresetRef::Timer(id))) => Ok((Some(id.0), None)),
        (Timer::Pomodoro(_), Some(PresetRef::Pomodoro(id))) => Ok((None, Some(id.0))),
        (_, Some(preset)) => Err(StoreError::Invalid {
            what: "preset of active timer",
            value: format!("{preset:?} for timer {}", active.id()),
        }),
    }
}

fn phase_to_sql(phase: Phase) -> &'static str {
    match phase {
        Phase::Focus => "focus",
        Phase::ShortBreak => "short_break",
        Phase::LongBreak => "long_break",
    }
}

/// Inserts or updates the timer row and replaces its segments (zero-length ones are dropped).
pub(super) fn save_active_tx(tx: &Transaction<'_>, active: &ActiveTimer) -> Result<(), StoreError> {
    let id = timer_id_to_sql(active.id())?;
    let c = Columns::of(active)?;
    tx.execute(
        "INSERT INTO active_timers (id, kind, task_id, timer_preset_id, pomodoro_preset_id,
             target_ms, focus_ms, short_break_ms, long_break_ms, focus_per_round, auto_start,
             phase, focus_index, round, phase_started)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, task_id = excluded.task_id,
             timer_preset_id = excluded.timer_preset_id,
             pomodoro_preset_id = excluded.pomodoro_preset_id, target_ms = excluded.target_ms,
             focus_ms = excluded.focus_ms, short_break_ms = excluded.short_break_ms,
             long_break_ms = excluded.long_break_ms, focus_per_round = excluded.focus_per_round,
             auto_start = excluded.auto_start, phase = excluded.phase,
             focus_index = excluded.focus_index, round = excluded.round,
             phase_started = excluded.phase_started",
        params![
            id,
            c.kind,
            active.task.map(|t| t.0),
            c.timer_preset,
            c.pomodoro_preset,
            c.target_ms,
            c.focus_ms,
            c.short_break_ms,
            c.long_break_ms,
            c.focus_per_round,
            c.auto_start,
            c.phase,
            c.focus_index,
            c.round,
            c.phase_started
        ],
    )?;
    tx.execute(
        "DELETE FROM active_timer_segments WHERE timer_id = ?1",
        [id],
    )?;
    if let Some(run) = c.run {
        insert_run(tx, id, run)?;
    }
    Ok(())
}

fn insert_run(tx: &Transaction<'_>, id: i64, run: &Run) -> Result<(), StoreError> {
    let mut insert = tx.prepare(
        "INSERT INTO active_timer_segments (timer_id, start_ms, end_ms) VALUES (?1, ?2, ?3)",
    )?;
    for segment in run.segments().iter().filter(|s| s.end > s.start) {
        let end = Some(segment.end.as_millis());
        insert.execute(params![id, segment.start.as_millis(), end])?;
    }
    if let Some(since) = run.running_since() {
        insert.execute(params![id, since.as_millis(), None::<i64>])?;
    }
    Ok(())
}

/// Deletes the timer row (its segments go with it); returns how many rows were deleted.
pub(super) fn delete_active_tx(tx: &Transaction<'_>, id: TimerId) -> Result<usize, StoreError> {
    let id = timer_id_to_sql(id)?;
    Ok(tx.execute("DELETE FROM active_timers WHERE id = ?1", [id])?)
}

impl Store {
    /// Deletes every active timer without recording anything: the way out when a saved timer
    /// is corrupt and the tracker cannot open (history is kept).
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn clear_active_timers(&mut self) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM active_timers", [])?;
        Ok(())
    }

    /// The saved active timers, by id, and the identifier the next timer must get.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored row is invalid (unknown
    /// kind or phase, missing column, inverted or overlapping segments, impossible pomodoro
    /// state).
    pub fn load_active(&self) -> Result<(u64, Vec<ActiveTimer>), StoreError> {
        load::load_all(&self.conn)
    }

    /// Active timers linked to `task`, by id ("Linked to").
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn active_timers_of(&self, task: TaskId) -> Result<Vec<ActiveTimerLink>, StoreError> {
        collect(
            &self.conn,
            "SELECT id, kind FROM active_timers WHERE task_id = ?1 ORDER BY id",
            [task.0],
            |row| {
                Ok(ActiveTimerLink {
                    timer: timer_id_from_sql(row.get(0)?)?,
                    kind: load::kind_from_sql(&row.get::<_, String>(1)?)?,
                })
            },
        )
    }
}

/// Single-row writes, for tests only: the tracker writes active timers through
/// `Store::write_timers`, so outside the crate memory and database cannot drift apart.
#[cfg(test)]
impl Store {
    /// Saves an active timer: inserts or updates its row and replaces its segments
    /// (zero-length closed segments are dropped), in one transaction.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] if its preset does not match its kind; [`StoreError::Sqlite`] if
    /// the task or preset does not exist or SQLite fails; [`StoreError::OutOfRange`] if the id or
    /// a duration does not fit an `INTEGER`.
    pub(crate) fn save_active(&mut self, active: &ActiveTimer) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        save_active_tx(&tx, active)?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes an active timer and its segments.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it is not saved.
    pub(crate) fn delete_active(&mut self, id: TimerId) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let deleted = delete_active_tx(&tx, id)?;
        tx.commit()?;
        super::rows::ensure_found(deleted, "active timer", timer_id_to_sql(id)?)
    }

    /// Runs raw SQL against the database (tests plant corrupt rows and failing triggers).
    pub(crate) fn execute_batch_for_tests(&self, sql: &str) {
        self.conn.execute_batch(sql).unwrap();
    }
}

#[cfg(test)]
mod tests;
