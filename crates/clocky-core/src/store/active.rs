//! Active timers: saved on every state change so they survive closing the app.
//!
//! Each timer is one `active_timers` row plus its run in `active_timer_segments` (the open
//! segment has `end_ms` NULL). Rows are read back through the engine's `from_parts`, so an
//! inconsistent row is an `Err`, never a panic.

use std::fmt::Display;

use rusqlite::{Connection, Row, Transaction, params};

use super::convert::u32_from_sql;
use super::convert::{duration_from_ms, duration_to_ms, timer_id_from_sql, timer_id_to_sql};
use super::rows::{collect, ensure_found};
use super::{Store, StoreError};
use crate::domain::{PomodoroPresetId, PresetRef, TaskId, TimerId, TimerPresetId};
use crate::engine::EngineError;
use crate::engine::countdown::Countdown;
use crate::engine::pomodoro::{Phase, Pomodoro, PomodoroConfig};
use crate::engine::run::{Run, Segment};
use crate::engine::session::TimerKind;
use crate::engine::stopwatch::Stopwatch;
use crate::engine::timers::{ActiveTimer, Timer};
use crate::time::Timestamp;

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
    preset: Option<i64>,
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
        let preset = preset_to_sql(active)?;
        Ok(match &active.timer {
            Timer::Stopwatch(sw) => Self {
                kind: "stopwatch",
                run: Some(sw.run()),
                ..Self::default()
            },
            Timer::Countdown(cd) => Self {
                kind: "countdown",
                preset,
                target_ms: Some(duration_to_ms(cd.target(), "countdown target")?),
                run: Some(cd.run()),
                ..Self::default()
            },
            Timer::Pomodoro(p) => Self::of_pomodoro(p, preset)?,
        })
    }

    fn of_pomodoro(p: &'a Pomodoro, preset: Option<i64>) -> Result<Self, StoreError> {
        let config = p.config();
        let phase_ms = |phase| duration_to_ms(config.duration(phase), "pomodoro phase");
        Ok(Self {
            kind: "pomodoro",
            preset,
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

fn preset_to_sql(active: &ActiveTimer) -> Result<Option<i64>, StoreError> {
    match (&active.timer, active.preset) {
        (_, None) => Ok(None),
        (Timer::Countdown(_), Some(PresetRef::Timer(id))) => Ok(Some(id.0)),
        (Timer::Pomodoro(_), Some(PresetRef::Pomodoro(id))) => Ok(Some(id.0)),
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

fn phase_from_sql(text: &str) -> Result<Phase, StoreError> {
    match text {
        "focus" => Ok(Phase::Focus),
        "short_break" => Ok(Phase::ShortBreak),
        "long_break" => Ok(Phase::LongBreak),
        _ => Err(corrupt("pomodoro phase", &text)),
    }
}

fn kind_from_sql(text: &str) -> Result<TimerKind, StoreError> {
    match text {
        "pomodoro" => Ok(TimerKind::Pomodoro),
        "stopwatch" => Ok(TimerKind::Stopwatch),
        "countdown" => Ok(TimerKind::Countdown),
        _ => Err(corrupt("active timer kind", &text)),
    }
}

fn corrupt(what: &'static str, value: &dyn Display) -> StoreError {
    StoreError::Corrupt {
        what,
        value: value.to_string(),
    }
}

/// A column that must not be NULL for this kind of timer.
fn required<T>(value: Option<T>, what: &'static str) -> Result<T, StoreError> {
    value.ok_or_else(|| corrupt(what, &"NULL"))
}

/// Inserts or updates the timer row and replaces its segments (zero-length ones are dropped).
pub(super) fn save_active_tx(tx: &Transaction<'_>, active: &ActiveTimer) -> Result<(), StoreError> {
    let id = timer_id_to_sql(active.id())?;
    let c = Columns::of(active)?;
    tx.execute(
        "INSERT INTO active_timers (id, kind, task_id, preset_id, target_ms, focus_ms,
             short_break_ms, long_break_ms, focus_per_round, auto_start, phase, focus_index,
             round, phase_started)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, task_id = excluded.task_id,
             preset_id = excluded.preset_id, target_ms = excluded.target_ms,
             focus_ms = excluded.focus_ms, short_break_ms = excluded.short_break_ms,
             long_break_ms = excluded.long_break_ms, focus_per_round = excluded.focus_per_round,
             auto_start = excluded.auto_start, phase = excluded.phase,
             focus_index = excluded.focus_index, round = excluded.round,
             phase_started = excluded.phase_started",
        params![
            id,
            c.kind,
            active.task.map(|t| t.0),
            c.preset,
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

/// An `active_timers` row as read, before it becomes an engine timer.
#[derive(Debug)]
struct ActiveRow {
    id: TimerId,
    kind: TimerKind,
    task: Option<TaskId>,
    preset: Option<i64>,
    target_ms: Option<i64>,
    pomodoro: PomodoroRow,
}

/// The pomodoro columns of an `active_timers` row (all NULL for the other kinds).
#[derive(Debug)]
struct PomodoroRow {
    focus_ms: Option<i64>,
    short_break_ms: Option<i64>,
    long_break_ms: Option<i64>,
    focus_per_round: Option<i64>,
    auto_start: Option<i64>,
    phase: Option<String>,
    focus_index: Option<i64>,
    round: Option<i64>,
    phase_started: Option<i64>,
}

const ACTIVE_COLUMNS: &str = "id, kind, task_id, preset_id, target_ms, focus_ms, \
     short_break_ms, long_break_ms, focus_per_round, auto_start, phase, focus_index, round, \
     phase_started";

fn active_row(row: &Row<'_>) -> Result<ActiveRow, StoreError> {
    Ok(ActiveRow {
        id: timer_id_from_sql(row.get(0)?)?,
        kind: kind_from_sql(&row.get::<_, String>(1)?)?,
        task: row.get::<_, Option<i64>>(2)?.map(TaskId),
        preset: row.get(3)?,
        target_ms: row.get(4)?,
        pomodoro: PomodoroRow {
            focus_ms: row.get(5)?,
            short_break_ms: row.get(6)?,
            long_break_ms: row.get(7)?,
            focus_per_round: row.get(8)?,
            auto_start: row.get(9)?,
            phase: row.get(10)?,
            focus_index: row.get(11)?,
            round: row.get(12)?,
            phase_started: row.get(13)?,
        },
    })
}

/// A stored segment: `end` is `None` for the open one.
type StoredSegment = (Timestamp, Option<Timestamp>);

fn segment_row(row: &Row<'_>) -> Result<StoredSegment, StoreError> {
    let start = Timestamp::from_millis(row.get(0)?);
    let end = row.get::<_, Option<i64>>(1)?.map(Timestamp::from_millis);
    Ok((start, end))
}

/// Maps an engine validation error on a stored timer to [`StoreError::Corrupt`].
fn engine_corrupt(id: TimerId) -> impl Fn(EngineError) -> StoreError {
    move |error| corrupt("active timer", &format!("{id}: {error}"))
}

fn run_from_segments(id: TimerId, segments: &[StoredSegment]) -> Result<Run, StoreError> {
    let closed: Vec<Segment> = segments
        .iter()
        .filter_map(|&(start, end)| end.map(|end| Segment { start, end }))
        .collect();
    let mut open = segments.iter().filter(|(_, end)| end.is_none());
    let running_since = open.next().map(|&(start, _)| start);
    if open.next().is_some() {
        return Err(corrupt("active timer", &format!("{id}: two open segments")));
    }
    Run::from_parts(closed, running_since).map_err(engine_corrupt(id))
}

fn pomodoro_from_row(
    id: TimerId,
    row: PomodoroRow,
    segments: &[StoredSegment],
) -> Result<Pomodoro, StoreError> {
    let flag = |value: Option<i64>, what| match required(value, what)? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(corrupt(what, &other)),
    };
    let config = PomodoroConfig::new(
        duration_from_ms(required(row.focus_ms, "pomodoro focus")?, "pomodoro focus")?,
        duration_from_ms(
            required(row.short_break_ms, "pomodoro short break")?,
            "short break",
        )?,
        duration_from_ms(
            required(row.long_break_ms, "pomodoro long break")?,
            "long break",
        )?,
        u32_from_sql(
            required(row.focus_per_round, "focus per round")?,
            "focus per round",
        )?,
        flag(row.auto_start, "pomodoro auto_start")?,
    )
    .map_err(engine_corrupt(id))?;
    let run = if flag(row.phase_started, "pomodoro phase_started")? {
        Some(run_from_segments(id, segments)?)
    } else if segments.is_empty() {
        None
    } else {
        return Err(corrupt(
            "active timer",
            &format!("{id}: waiting phase has segments"),
        ));
    };
    Pomodoro::from_parts(
        config,
        phase_from_sql(&required(row.phase, "pomodoro phase")?)?,
        u32_from_sql(required(row.focus_index, "focus index")?, "focus index")?,
        u32_from_sql(required(row.round, "pomodoro round")?, "pomodoro round")?,
        run,
    )
    .map_err(engine_corrupt(id))
}

fn timer_from_row(row: ActiveRow, segments: &[StoredSegment]) -> Result<ActiveTimer, StoreError> {
    let id = row.id;
    let (timer, preset) = match row.kind {
        TimerKind::Stopwatch => (
            Timer::Stopwatch(Stopwatch::from_parts(run_from_segments(id, segments)?)),
            None,
        ),
        TimerKind::Countdown => {
            let target = duration_from_ms(required(row.target_ms, "countdown target")?, "target")?;
            let run = run_from_segments(id, segments)?;
            let preset = row.preset.map(|p| PresetRef::Timer(TimerPresetId(p)));
            (Timer::Countdown(Countdown::from_parts(run, target)), preset)
        }
        TimerKind::Pomodoro => {
            let pomodoro = pomodoro_from_row(id, row.pomodoro, segments)?;
            let preset = row.preset.map(|p| PresetRef::Pomodoro(PomodoroPresetId(p)));
            (Timer::Pomodoro(pomodoro), preset)
        }
    };
    Ok(ActiveTimer::new(id, row.task, preset, timer))
}

/// The identifier the next timer gets: one more than any id used by an active timer, a session
/// or a subtask event (decision 5, so a run id is never reused).
fn next_timer_id(conn: &Connection) -> Result<u64, StoreError> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(id) FROM (
             SELECT MAX(id) AS id FROM active_timers
             UNION ALL SELECT MAX(run_id) FROM sessions
             UNION ALL SELECT MAX(run_id) FROM subtask_events)",
        [],
        |row| row.get(0),
    )?;
    let Some(max) = max else {
        return Ok(1);
    };
    timer_id_from_sql(max)?
        .0
        .checked_add(1)
        .ok_or(StoreError::OutOfRange("timer id"))
}

impl Store {
    /// Saves an active timer: inserts or updates its row and replaces its segments
    /// (zero-length closed segments are dropped), in one transaction.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] if its preset does not match its kind; [`StoreError::Sqlite`] if
    /// the task does not exist or SQLite fails; [`StoreError::OutOfRange`] if the id or a
    /// duration does not fit an `INTEGER`.
    pub fn save_active(&mut self, active: &ActiveTimer) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        save_active_tx(&tx, active)?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes an active timer and its segments.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it is not saved.
    pub fn delete_active(&mut self, id: TimerId) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let deleted = delete_active_tx(&tx, id)?;
        tx.commit()?;
        ensure_found(deleted, "active timer", timer_id_to_sql(id)?)
    }

    /// The saved active timers, by id, and the identifier the next timer must get.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored row is invalid (unknown
    /// kind or phase, missing column, inverted or overlapping segments, impossible pomodoro
    /// state).
    pub fn load_active(&self) -> Result<(u64, Vec<ActiveTimer>), StoreError> {
        let sql = format!("SELECT {ACTIVE_COLUMNS} FROM active_timers ORDER BY id");
        let rows = collect(&self.conn, &sql, [], active_row)?;
        let mut timers = Vec::with_capacity(rows.len());
        for row in rows {
            let segments = collect(
                &self.conn,
                "SELECT start_ms, end_ms FROM active_timer_segments
                 WHERE timer_id = ?1 ORDER BY start_ms, end_ms IS NULL",
                [timer_id_to_sql(row.id)?],
                segment_row,
            )?;
            timers.push(timer_from_row(row, &segments)?);
        }
        Ok((next_timer_id(&self.conn)?, timers))
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
                    kind: kind_from_sql(&row.get::<_, String>(1)?)?,
                })
            },
        )
    }
}

#[cfg(test)]
impl Store {
    /// Runs raw SQL against the database (tests plant corrupt rows and failing triggers).
    pub(crate) fn execute_batch_for_tests(&self, sql: &str) {
        self.conn.execute_batch(sql).unwrap();
    }
}

#[cfg(test)]
mod tests;
