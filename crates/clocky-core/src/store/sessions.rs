//! Recorded sessions: segments, links to subtask events, edits and history.

use std::time::Duration;

use rusqlite::{Connection, Row, Transaction, params};

use super::convert::{
    duration_from_ms, duration_to_ms, timer_id_from_sql, timer_id_to_sql, u32_from_sql,
};
use super::rows::{collect, ensure_found, first};
use super::subtasks::{EVENT_COLUMNS, event_from_row};
use super::{Store, StoreError, SubtaskEvent};
use crate::domain::{SessionId, TaskId, TimerId};
use crate::engine::run::Segment;
use crate::engine::session::{CompletedSession, TimerKind};
use crate::time::Timestamp;

/// A recorded session (a pomodoro focus phase, a stopwatch or a countdown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Identifier.
    pub id: SessionId,
    /// Kind of timer.
    pub kind: TimerKind,
    /// Task; `None` = "No task".
    pub task: Option<TaskId>,
    /// Timer run that produced it (groups the sessions of a pomodoro).
    pub run: TimerId,
    /// Pomodoro round (`None` for the other kinds).
    pub round: Option<u32>,
    /// Start of the first segment.
    pub started_at: Timestamp,
    /// End of the last segment.
    pub ended_at: Timestamp,
    /// Effective time (sum of the segments).
    pub effective: Duration,
}

/// A session with its segments and the subtask events that happened during it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDetail {
    /// The session.
    pub session: Session,
    /// Segments in which the timer ran; the gaps are the pauses.
    pub segments: Vec<Segment>,
    /// Linked subtask events, oldest first.
    pub events: Vec<SubtaskEvent>,
}

/// Columns read by [`session_from_row`], for a `sessions` table aliased as `s`.
pub(super) const SESSION_COLUMNS: &str = "s.id, s.kind, s.task_id, s.run_id, s.round, \
     s.started_at_ms, s.ended_at_ms, s.effective_ms";

/// Reads the [`SESSION_COLUMNS`] at the start of `row`.
pub(super) fn session_from_row(row: &Row<'_>) -> Result<Session, StoreError> {
    Ok(Session {
        id: SessionId(row.get(0)?),
        kind: kind_from_sql(row.get(1)?)?,
        task: row.get::<_, Option<i64>>(2)?.map(TaskId),
        run: timer_id_from_sql(row.get(3)?)?,
        round: row
            .get::<_, Option<i64>>(4)?
            .map(|round| u32_from_sql(round, "session round"))
            .transpose()?,
        started_at: Timestamp::from_millis(row.get(5)?),
        ended_at: Timestamp::from_millis(row.get(6)?),
        effective: duration_from_ms(row.get(7)?, "session duration")?,
    })
}

fn kind_to_sql(kind: TimerKind) -> &'static str {
    match kind {
        TimerKind::Pomodoro => "pomodoro",
        TimerKind::Stopwatch => "stopwatch",
        TimerKind::Countdown => "countdown",
    }
}

fn kind_from_sql(text: String) -> Result<TimerKind, StoreError> {
    match text.as_str() {
        "pomodoro" => Ok(TimerKind::Pomodoro),
        "stopwatch" => Ok(TimerKind::Stopwatch),
        "countdown" => Ok(TimerKind::Countdown),
        _ => Err(StoreError::Corrupt {
            what: "session kind",
            value: text,
        }),
    }
}

fn segment_from_row(row: &Row<'_>) -> Result<Segment, StoreError> {
    let start: i64 = row.get(0)?;
    let end: i64 = row.get(1)?;
    if end <= start {
        return Err(StoreError::Corrupt {
            what: "session segment",
            value: format!("{start}..{end}"),
        });
    }
    Ok(Segment {
        start: Timestamp::from_millis(start),
        end: Timestamp::from_millis(end),
    })
}

/// Inserts the segments of session `id`.
fn insert_segments(conn: &Connection, id: i64, segments: &[Segment]) -> Result<(), StoreError> {
    let mut insert = conn.prepare(
        "INSERT INTO session_segments (session_id, start_ms, end_ms) VALUES (?1, ?2, ?3)",
    )?;
    for segment in segments {
        insert.execute(params![
            id,
            segment.start.as_millis(),
            segment.end.as_millis()
        ])?;
    }
    Ok(())
}

impl Store {
    /// Records a session of timer `run` in one transaction: the session, its segments
    /// (zero-length ones are dropped) and the link to the subtask events of `run` that happened
    /// inside `[started_at, ended_at]` and are not linked yet.
    ///
    /// `started_at`, `ended_at` and `effective` are computed from the segments that are kept, so
    /// a zero-length segment at either edge does not widen the session.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if the task does not exist or SQLite fails (nothing is recorded);
    /// [`StoreError::OutOfRange`] if `run` or the duration does not fit an `INTEGER`;
    /// [`StoreError::EmptySession`] if no segment has a duration (a [`CompletedSession`] built
    /// by the engine always has one).
    pub fn record_session(
        &mut self,
        task: Option<TaskId>,
        run: TimerId,
        round: Option<u32>,
        session: &CompletedSession,
    ) -> Result<SessionId, StoreError> {
        let tx = self.conn.transaction()?;
        let id = record_session_tx(&tx, task, run, round, session)?;
        tx.commit()?;
        Ok(id)
    }
}

/// [`Store::record_session`] inside the caller's transaction (the tracker also updates
/// `active_timers` in it).
pub(crate) fn record_session_tx(
    tx: &Transaction<'_>,
    task: Option<TaskId>,
    run: TimerId,
    round: Option<u32>,
    session: &CompletedSession,
) -> Result<SessionId, StoreError> {
    let run_id = timer_id_to_sql(run)?;
    let segments: Vec<Segment> = session
        .segments()
        .iter()
        .copied()
        .filter(|segment| segment.end > segment.start)
        .collect();
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        return Err(StoreError::EmptySession);
    };
    let (started, ended) = (first.start.as_millis(), last.end.as_millis());
    let effective: Duration = segments.iter().map(Segment::duration).sum();
    let effective = duration_to_ms(effective, "session duration")?;
    tx.execute(
        "INSERT INTO sessions (kind, task_id, run_id, round, started_at_ms, ended_at_ms,
             effective_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            kind_to_sql(session.kind()),
            task.map(|t| t.0),
            run_id,
            round,
            started,
            ended,
            effective
        ],
    )?;
    let id = tx.last_insert_rowid();
    insert_segments(tx, id, &segments)?;
    tx.execute(
        "UPDATE subtask_events SET session_id = ?1
         WHERE run_id = ?2 AND session_id IS NULL AND at_ms BETWEEN ?3 AND ?4",
        params![id, run_id, started, ended],
    )?;
    Ok(SessionId(id))
}

impl Store {
    /// The session with its segments and linked subtask events.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist; [`StoreError::Corrupt`] or
    /// [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn session(&self, id: SessionId) -> Result<SessionDetail, StoreError> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions s WHERE s.id = ?1");
        let session = first(
            collect(&self.conn, &sql, [id.0], session_from_row)?,
            "session",
            id.0,
        )?;
        let segments = collect(
            &self.conn,
            "SELECT start_ms, end_ms FROM session_segments WHERE session_id = ?1 ORDER BY start_ms",
            [id.0],
            segment_from_row,
        )?;
        let sql = format!(
            "SELECT {EVENT_COLUMNS} FROM subtask_events e
             WHERE e.session_id = ?1 ORDER BY e.at_ms, e.id"
        );
        let events = collect(&self.conn, &sql, [id.0], event_from_row)?;
        Ok(SessionDetail {
            session,
            segments,
            events,
        })
    }
}

impl Store {
    /// Deletes a session and its segments; its subtask events stay, without a session.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn delete_session(&mut self, id: SessionId) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM sessions WHERE id = ?1", [id.0])?;
        ensure_found(changed, "session", id.0)
    }

    /// Changes only the task of a session (`None` = "No task").
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the session does not exist; [`StoreError::Sqlite`] if the
    /// task does not exist.
    pub fn set_session_task(
        &mut self,
        id: SessionId,
        task: Option<TaskId>,
    ) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE sessions SET task_id = ?1 WHERE id = ?2",
            params![task.map(|t| t.0), id.0],
        )?;
        ensure_found(changed, "session", id.0)
    }

    /// The latest `limit` sessions of `task`, newest first.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn recent_sessions(&self, task: TaskId, limit: u32) -> Result<Vec<Session>, StoreError> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM sessions s WHERE s.task_id = ?1
             ORDER BY s.started_at_ms DESC, s.id DESC LIMIT ?2"
        );
        collect(&self.conn, &sql, params![task.0, limit], session_from_row)
    }
}

#[cfg(test)]
mod tests;
