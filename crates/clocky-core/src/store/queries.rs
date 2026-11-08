//! Read-only queries for the UI: sidebar, task cards, "Linked to", "Tracked time" and the task
//! picker. Archived projects and tasks are left out.

use std::time::Duration;

use rusqlite::params;

use super::convert::{duration_from_ms, u32_from_sql};
use super::projects::{PROJECT_COLUMNS, project_from_row};
use super::rows::collect;
use super::tasks::{TASK_COLUMNS, task_from_row};
use super::{PomodoroPreset, Project, Store, StoreError, Task, TimerPreset};
use crate::domain::{ProjectId, TaskId};

/// A project with its task counts (sidebar and cards).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOverview {
    /// The project.
    pub project: Project,
    /// Tasks not completed (archived ones are not counted).
    pub open_tasks: u32,
    /// Completed tasks (archived ones are not counted).
    pub completed_tasks: u32,
}

/// What a task is linked to ("Linked to").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskLinks {
    /// Saved countdown timers of the task.
    pub timer_presets: Vec<TimerPreset>,
    /// Saved pomodoros of the task.
    pub pomodoro_presets: Vec<PomodoroPreset>,
}

/// A task card: the task, its checklist progress, tracked time and links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    /// The task.
    pub task: Task,
    /// Completed subtasks (the `n` of `n/m`).
    pub subtasks_done: u32,
    /// All subtasks (the `m` of `n/m`).
    pub subtasks_total: u32,
    /// Sum of the effective time of its sessions.
    pub tracked: Duration,
    /// Linked presets.
    pub links: TaskLinks,
}

impl Store {
    /// Active projects in creation order, with their open and completed task counts.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn projects_overview(&self) -> Result<Vec<ProjectOverview>, StoreError> {
        let sql = format!(
            "SELECT {PROJECT_COLUMNS}, COUNT(t.id) - COUNT(t.completed_at_ms), COUNT(t.completed_at_ms)
             FROM projects p
             LEFT JOIN tasks t ON t.project_id = p.id AND t.archived_at_ms IS NULL
             WHERE p.archived_at_ms IS NULL
             GROUP BY p.id
             ORDER BY p.created_at_ms, p.id"
        );
        collect(&self.conn, &sql, [], |row| {
            Ok(ProjectOverview {
                project: project_from_row(row)?,
                open_tasks: u32_from_sql(row.get(5)?, "open task count")?,
                completed_tasks: u32_from_sql(row.get(6)?, "completed task count")?,
            })
        })
    }

    /// Task cards of `project` (`None` = tasks without a project), in order. Completed tasks
    /// only if `include_completed`.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn task_summaries(
        &self,
        project: Option<ProjectId>,
        include_completed: bool,
    ) -> Result<Vec<TaskSummary>, StoreError> {
        let sql = format!(
            "SELECT {TASK_COLUMNS},
                 (SELECT COUNT(*) FROM subtasks st
                  WHERE st.task_id = t.id AND st.completed_at_ms IS NOT NULL),
                 (SELECT COUNT(*) FROM subtasks st WHERE st.task_id = t.id),
                 (SELECT COALESCE(SUM(s.effective_ms), 0) FROM sessions s WHERE s.task_id = t.id)
             FROM tasks t
             WHERE t.project_id IS ?1 AND t.archived_at_ms IS NULL
                 AND (?2 OR t.completed_at_ms IS NULL)
             ORDER BY t.position, t.id"
        );
        let params = params![project.map(|p| p.0), include_completed];
        collect(&self.conn, &sql, params, |row| {
            let task = task_from_row(row)?;
            let links = self.task_links(task.id)?;
            Ok(TaskSummary {
                task,
                subtasks_done: u32_from_sql(row.get(7)?, "subtask count")?,
                subtasks_total: u32_from_sql(row.get(8)?, "subtask count")?,
                tracked: duration_from_ms(row.get(9)?, "tracked time")?,
                links,
            })
        })
    }

    /// Presets linked to `task` ("Linked to").
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn task_links(&self, task: TaskId) -> Result<TaskLinks, StoreError> {
        Ok(TaskLinks {
            timer_presets: self.timer_presets_of(Some(task))?,
            pomodoro_presets: self.pomodoro_presets_of(Some(task))?,
        })
    }

    /// Total effective time recorded for `task` ("Tracked time").
    ///
    /// # Errors
    /// [`StoreError::OutOfRange`] if the stored total is negative.
    pub fn task_time(&self, task: TaskId) -> Result<Duration, StoreError> {
        let millis: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(effective_ms), 0) FROM sessions WHERE task_id = ?1",
            [task.0],
            |row| row.get(0),
        )?;
        duration_from_ms(millis, "tracked time")
    }
}

impl Store {
    /// The latest `limit` tasks with sessions, by their last session ("Recent" in the picker).
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn recent_tasks(&self, limit: u32) -> Result<Vec<Task>, StoreError> {
        let sql = format!(
            "SELECT {TASK_COLUMNS} FROM tasks t
             JOIN sessions s ON s.task_id = t.id
             LEFT JOIN projects p ON p.id = t.project_id
             WHERE t.archived_at_ms IS NULL AND p.archived_at_ms IS NULL
             GROUP BY t.id
             ORDER BY MAX(s.ended_at_ms) DESC, t.id
             LIMIT ?1"
        );
        collect(&self.conn, &sql, [limit], task_from_row)
    }

    /// Tasks whose name or project name contains `text` (task picker), by name.
    ///
    /// Case is ignored for ASCII letters only (SQLite `lower`), so accented letters must match
    /// exactly.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn search_tasks(&self, text: &str) -> Result<Vec<Task>, StoreError> {
        let sql = format!(
            "SELECT {TASK_COLUMNS} FROM tasks t
             LEFT JOIN projects p ON p.id = t.project_id
             WHERE t.archived_at_ms IS NULL AND p.archived_at_ms IS NULL
                 AND (instr(lower(t.name), lower(?1)) > 0
                      OR instr(lower(COALESCE(p.name, '')), lower(?1)) > 0)
             ORDER BY t.name, t.id"
        );
        collect(&self.conn, &sql, [text], task_from_row)
    }
}

#[cfg(test)]
mod tests;
