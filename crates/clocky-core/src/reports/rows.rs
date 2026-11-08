//! SQL reads shared by the reports, and the labels they carry (task, project and color).

use std::cmp::Reverse;
use std::time::Duration;

use rusqlite::{Params, Row};

use super::{Period, ReportError};
use crate::domain::{ProjectId, SessionId, SubtaskId, TaskId};
use crate::engine::run::Segment;
use crate::engine::session::TimerKind;
use crate::store::{Store, StoreError, SubtaskEventKind};
use crate::time::Timestamp;

/// A project as shown in the reports. Archived projects are included: they are history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    /// Identifier.
    pub id: ProjectId,
    /// Name.
    pub name: String,
    /// Color chosen in the UI (for example `#7c5cff`).
    pub color: String,
    /// Whether the project is archived.
    pub archived: bool,
}

/// A task as shown in the reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInfo {
    /// Identifier.
    pub id: TaskId,
    /// Name.
    pub name: String,
}

/// A subtask checked or unchecked, as a marker on the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskMarker {
    /// The subtask.
    pub subtask: SubtaskId,
    /// Its name.
    pub name: String,
    /// Its task.
    pub task: TaskInfo,
    /// Project of that task; `None` = "No project".
    pub project: Option<ProjectInfo>,
    /// Checked or unchecked.
    pub kind: SubtaskEventKind,
    /// When.
    pub at: Timestamp,
    /// Session that contains it; `None` if it happened outside a recorded session (for
    /// example during a pomodoro break, or with no timer).
    ///
    /// The session may have no block on the same day: an event during a pause that covers
    /// the whole day still belongs to its session, which ran only on other days.
    pub session: Option<SessionId>,
}

/// A recorded segment with the session it belongs to (not clipped).
#[derive(Debug, Clone)]
pub(super) struct SegmentRow {
    /// Session of the segment.
    pub session: SessionId,
    /// Kind of timer of the session.
    pub kind: TimerKind,
    /// Pomodoro round of the session.
    pub round: Option<u32>,
    /// Task of the session; `None` = "No task".
    pub task: Option<TaskInfo>,
    /// Project of that task; `None` = "No project".
    pub project: Option<ProjectInfo>,
    /// The segment as recorded.
    pub segment: Segment,
}

/// Every segment that overlaps `period`, oldest first, with its session, task and project.
pub(super) fn segments_overlapping(
    store: &Store,
    period: Period,
) -> Result<Vec<SegmentRow>, ReportError> {
    let sql = "SELECT g.session_id, s.kind, s.round, t.id, t.name,
                   p.id, p.name, p.color, p.archived_at_ms, g.start_ms, g.end_ms
               FROM session_segments g
               JOIN sessions s ON s.id = g.session_id
               LEFT JOIN tasks t ON t.id = s.task_id
               LEFT JOIN projects p ON p.id = t.project_id
               WHERE g.start_ms < ?2 AND g.end_ms > ?1
                   AND s.started_at_ms < ?2 AND s.ended_at_ms > ?1
               ORDER BY g.start_ms, g.session_id";
    let bounds = [period.start().as_millis(), period.end().as_millis()];
    collect(store, sql, bounds, |row| {
        Ok(SegmentRow {
            session: SessionId(row.get(0)?),
            kind: kind_from_sql(row.get(1)?)?,
            round: round_from_sql(row.get(2)?)?,
            task: task_at(row, 3)?,
            project: project_at(row, 5)?,
            segment: Segment {
                start: Timestamp::from_millis(row.get(9)?),
                end: Timestamp::from_millis(row.get(10)?),
            },
        })
    })
}

/// Subtask events that match `filter` (an SQL condition on `e`, with `params`), oldest first.
pub(super) fn subtask_markers<P: Params>(
    store: &Store,
    filter: &str,
    params: P,
) -> Result<Vec<SubtaskMarker>, ReportError> {
    let sql = format!(
        "SELECT e.subtask_id, st.name, e.kind, e.at_ms, e.session_id, t.id, t.name,
             p.id, p.name, p.color, p.archived_at_ms
         FROM subtask_events e
         JOIN subtasks st ON st.id = e.subtask_id
         JOIN tasks t ON t.id = st.task_id
         LEFT JOIN projects p ON p.id = t.project_id
         WHERE {filter}
         ORDER BY e.at_ms, e.id"
    );
    collect(store, &sql, params, |row| {
        Ok(SubtaskMarker {
            subtask: SubtaskId(row.get(0)?),
            name: row.get(1)?,
            kind: event_kind_from_sql(row.get(2)?)?,
            at: Timestamp::from_millis(row.get(3)?),
            session: row.get::<_, Option<i64>>(4)?.map(SessionId),
            task: TaskInfo {
                id: TaskId(row.get(5)?),
                name: row.get(6)?,
            },
            project: project_at(row, 7)?,
        })
    })
}

/// Runs a query and converts each row with `convert`.
pub(super) fn collect<T, P: Params>(
    store: &Store,
    sql: &str,
    params: P,
    mut convert: impl FnMut(&Row<'_>) -> Result<T, ReportError>,
) -> Result<Vec<T>, ReportError> {
    let mut stmt = store.conn().prepare(sql)?;
    let mut rows = stmt.query(params)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(convert(row)?);
    }
    Ok(items)
}

/// The task in the columns `id, name` starting at `index` (`NULL` id = no task).
pub(super) fn task_at(row: &Row<'_>, index: usize) -> Result<Option<TaskInfo>, ReportError> {
    let id: Option<i64> = row.get(index)?;
    let name: Option<String> = row.get(index + 1)?;
    Ok(id.zip(name).map(|(id, name)| TaskInfo {
        id: TaskId(id),
        name,
    }))
}

/// The project in the columns `id, name, color, archived_at_ms` starting at `index`.
pub(super) fn project_at(row: &Row<'_>, index: usize) -> Result<Option<ProjectInfo>, ReportError> {
    let id: Option<i64> = row.get(index)?;
    let name: Option<String> = row.get(index + 1)?;
    let color: Option<String> = row.get(index + 2)?;
    let archived: Option<i64> = row.get(index + 3)?;
    Ok(id
        .zip(name)
        .zip(color)
        .map(|((id, name), color)| ProjectInfo {
            id: ProjectId(id),
            name,
            color,
            archived: archived.is_some(),
        }))
}

fn kind_from_sql(text: String) -> Result<TimerKind, ReportError> {
    match text.as_str() {
        "pomodoro" => Ok(TimerKind::Pomodoro),
        "stopwatch" => Ok(TimerKind::Stopwatch),
        "countdown" => Ok(TimerKind::Countdown),
        _ => Err(corrupt("session kind", text)),
    }
}

fn event_kind_from_sql(text: String) -> Result<SubtaskEventKind, ReportError> {
    match text.as_str() {
        "completed" => Ok(SubtaskEventKind::Completed),
        "reopened" => Ok(SubtaskEventKind::Reopened),
        _ => Err(corrupt("subtask event kind", text)),
    }
}

fn round_from_sql(round: Option<i64>) -> Result<Option<u32>, ReportError> {
    round
        .map(|round| u32::try_from(round).map_err(|_| StoreError::OutOfRange("session round")))
        .transpose()
        .map_err(ReportError::from)
}

/// A [`StoreError::Corrupt`] for an invalid stored value.
fn corrupt(what: &'static str, value: String) -> ReportError {
    ReportError::Store(StoreError::Corrupt { what, value })
}

/// `total + part`, saturating instead of overflowing.
pub(super) fn add(total: &mut Duration, part: Duration) {
    *total = total.saturating_add(part);
}

/// Sorts by total time descending; ties by id ascending, with the "No project"/"No task" entry
/// (`None` id) last.
pub(super) fn sort_by_total<T>(items: &mut [T], key: impl Fn(&T) -> (Duration, Option<i64>)) {
    items.sort_by_key(|item| {
        let (total, id) = key(item);
        (Reverse(total), id.is_none(), id)
    });
}
