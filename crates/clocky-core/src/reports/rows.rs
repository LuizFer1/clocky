//! SQL reads shared by the reports, and the labels they carry (task, project and color).

use std::cmp::Reverse;
use std::time::Duration;

use rusqlite::{Params, Row};

use super::{Period, ReportError};
use crate::domain::{ProjectId, TaskId};
use crate::engine::run::Segment;
use crate::store::Store;
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

/// A recorded segment with the session it belongs to (not clipped).
#[derive(Debug, Clone)]
pub(super) struct SegmentRow {
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
    let sql = "SELECT t.id, t.name,
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
            task: task_at(row, 0)?,
            project: project_at(row, 2)?,
            segment: Segment {
                start: Timestamp::from_millis(row.get(6)?),
                end: Timestamp::from_millis(row.get(7)?),
            },
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
