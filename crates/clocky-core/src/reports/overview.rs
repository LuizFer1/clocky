//! Overview ("Geral"): time per project, then per task, in a period.

use std::collections::BTreeMap;
use std::time::Duration;

use super::rows::{ProjectInfo, TaskInfo, add, segments_overlapping, sort_by_total};
use super::{Period, ReportError};
use crate::domain::{ProjectId, TaskId};
use crate::store::Store;

/// Time per project and task in a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    /// The period.
    pub period: Period,
    /// Time inside the period, all projects together.
    pub total: Duration,
    /// Projects with time, by time descending. The "No project" group has `project: None`.
    pub projects: Vec<ProjectTotal>,
}

/// Time of one project in a period, split by task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTotal {
    /// The project; `None` = "No project".
    pub project: Option<ProjectInfo>,
    /// Time of the project.
    pub total: Duration,
    /// Tasks with time, by time descending. The "No task" row has `task: None` and is always
    /// in the "No project" group.
    pub tasks: Vec<TaskTotal>,
}

/// Time of one task in a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTotal {
    /// The task; `None` = "No task" (sessions without a task, or whose task was deleted).
    pub task: Option<TaskInfo>,
    /// Time of the task.
    pub total: Duration,
}

/// Time per project and task in `period`, from the segments clipped to it (a session that
/// crosses a boundary counts only its part inside).
///
/// # Errors
/// [`ReportError::Store`] if reading the database fails or a stored value is invalid.
pub fn overview(store: &Store, period: Period) -> Result<Overview, ReportError> {
    let mut groups: BTreeMap<Option<ProjectId>, Group> = BTreeMap::new();
    let mut total = Duration::ZERO;
    for row in segments_overlapping(store, period)? {
        let Some(part) = period.clip(row.segment) else {
            continue;
        };
        let time = part.duration();
        add(&mut total, time);
        let group = groups
            .entry(row.project.as_ref().map(|project| project.id))
            .or_insert_with(|| Group::new(row.project));
        add(&mut group.total, time);
        let task = group
            .tasks
            .entry(row.task.as_ref().map(|task| task.id))
            .or_insert_with(|| TaskTotal {
                task: row.task,
                total: Duration::ZERO,
            });
        add(&mut task.total, time);
    }
    let mut projects: Vec<ProjectTotal> = groups.into_values().map(Group::finish).collect();
    sort_by_total(&mut projects, |group| {
        (
            group.total,
            group.project.as_ref().map(|project| project.id.0),
        )
    });
    Ok(Overview {
        period,
        total,
        projects,
    })
}

/// A project being added up.
struct Group {
    project: Option<ProjectInfo>,
    total: Duration,
    tasks: BTreeMap<Option<TaskId>, TaskTotal>,
}

impl Group {
    fn new(project: Option<ProjectInfo>) -> Self {
        Self {
            project,
            total: Duration::ZERO,
            tasks: BTreeMap::new(),
        }
    }

    fn finish(self) -> ProjectTotal {
        let mut tasks: Vec<TaskTotal> = self.tasks.into_values().collect();
        sort_by_total(&mut tasks, |row| {
            (row.total, row.task.as_ref().map(|task| task.id.0))
        });
        ProjectTotal {
            project: self.project,
            total: self.total,
            tasks,
        }
    }
}

#[cfg(test)]
mod tests;
