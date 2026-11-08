//! Reading active timers back: each row and its segments go through the engine's
//! `from_parts`, so an inconsistent row is a [`StoreError::Corrupt`], never a panic.

use std::fmt::Display;

use rusqlite::{Connection, Row};

use crate::domain::{PomodoroPresetId, PresetRef, TaskId, TimerId, TimerPresetId};
use crate::engine::EngineError;
use crate::engine::countdown::Countdown;
use crate::engine::pomodoro::{Phase, Pomodoro, PomodoroConfig};
use crate::engine::run::{Run, Segment};
use crate::engine::session::TimerKind;
use crate::engine::stopwatch::Stopwatch;
use crate::engine::timers::{ActiveTimer, Timer};
use crate::store::StoreError;
use crate::store::convert::{duration_from_ms, timer_id_from_sql, timer_id_to_sql, u32_from_sql};
use crate::store::rows::collect;
use crate::time::Timestamp;

fn phase_from_sql(text: &str) -> Result<Phase, StoreError> {
    match text {
        "focus" => Ok(Phase::Focus),
        "short_break" => Ok(Phase::ShortBreak),
        "long_break" => Ok(Phase::LongBreak),
        _ => Err(corrupt("pomodoro phase", &text)),
    }
}

pub(super) fn kind_from_sql(text: &str) -> Result<TimerKind, StoreError> {
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

/// Every saved timer, by id, and the identifier the next timer must get.
pub(super) fn load_all(conn: &Connection) -> Result<(u64, Vec<ActiveTimer>), StoreError> {
    let sql = format!("SELECT {ACTIVE_COLUMNS} FROM active_timers ORDER BY id");
    let rows = collect(conn, &sql, [], active_row)?;
    let mut timers = Vec::with_capacity(rows.len());
    for row in rows {
        let segments = collect(
            conn,
            "SELECT start_ms, end_ms FROM active_timer_segments
             WHERE timer_id = ?1 ORDER BY start_ms, end_ms IS NULL",
            [timer_id_to_sql(row.id)?],
            segment_row,
        )?;
        timers.push(timer_from_row(row, &segments)?);
    }
    Ok((next_timer_id(conn)?, timers))
}
