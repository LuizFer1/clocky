//! Tasks: create (appended to the end of their project), rename, complete, archive, move and
//! delete.

use rusqlite::{Row, params};

use super::rows::{collect, ensure_found, first};
use super::{Store, StoreError};
use crate::domain::{ProjectId, TaskId};
use crate::time::Timestamp;

/// A task, optionally inside a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Identifier.
    pub id: TaskId,
    /// Project; `None` = "No project".
    pub project: Option<ProjectId>,
    /// Name.
    pub name: String,
    /// When it was completed; `None` while open.
    pub completed_at: Option<Timestamp>,
    /// When it was archived; `None` while active.
    pub archived_at: Option<Timestamp>,
    /// Order inside the project (ascending).
    pub position: i64,
    /// When it was created.
    pub created_at: Timestamp,
}

/// Columns read by [`task_from_row`], for a `tasks` table aliased as `t`.
pub(super) const TASK_COLUMNS: &str = "t.id, t.project_id, t.name, t.completed_at_ms, \
     t.archived_at_ms, t.position, t.created_at_ms";

/// Reads the [`TASK_COLUMNS`] at the start of `row`.
pub(super) fn task_from_row(row: &Row<'_>) -> Result<Task, StoreError> {
    Ok(Task {
        id: TaskId(row.get(0)?),
        project: row.get::<_, Option<i64>>(1)?.map(ProjectId),
        name: row.get(2)?,
        completed_at: row.get::<_, Option<i64>>(3)?.map(Timestamp::from_millis),
        archived_at: row.get::<_, Option<i64>>(4)?.map(Timestamp::from_millis),
        position: row.get(5)?,
        created_at: Timestamp::from_millis(row.get(6)?),
    })
}

impl Store {
    /// Creates a task at the end of `project` (`None` = no project).
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if the project does not exist or SQLite fails.
    pub fn create_task(
        &mut self,
        project: Option<ProjectId>,
        name: &str,
        now: Timestamp,
    ) -> Result<TaskId, StoreError> {
        self.conn.execute(
            "INSERT INTO tasks (project_id, name, position, created_at_ms)
             SELECT ?1, ?2, COALESCE(MAX(position), 0) + 1, ?3 FROM tasks WHERE project_id IS ?1",
            params![project.map(|p| p.0), name, now.as_millis()],
        )?;
        Ok(TaskId(self.conn.last_insert_rowid()))
    }

    /// The task with this identifier.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn task(&self, id: TaskId) -> Result<Task, StoreError> {
        let sql = format!("SELECT {TASK_COLUMNS} FROM tasks t WHERE t.id = ?1");
        first(
            collect(&self.conn, &sql, [id.0], task_from_row)?,
            "task",
            id.0,
        )
    }

    /// Renames a task.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn rename_task(&mut self, id: TaskId, name: &str) -> Result<(), StoreError> {
        self.update_task(id, "UPDATE tasks SET name = ?2 WHERE id = ?1", name)
    }

    /// Marks a task as completed (completing its subtasks is up to the user). If it is already
    /// completed, the original time is kept.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn complete_task(&mut self, id: TaskId, now: Timestamp) -> Result<(), StoreError> {
        let sql = "UPDATE tasks SET completed_at_ms = COALESCE(completed_at_ms, ?2) WHERE id = ?1";
        self.update_task(id, sql, now.as_millis())
    }

    /// Reopens a completed task.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn reopen_task(&mut self, id: TaskId) -> Result<(), StoreError> {
        let sql = "UPDATE tasks SET completed_at_ms = NULL WHERE id = ?1";
        let changed = self.conn.execute(sql, [id.0])?;
        ensure_found(changed, "task", id.0)
    }

    /// Archives a task: it leaves the lists, and its history stays in the reports. If it is
    /// already archived, the original time is kept.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn archive_task(&mut self, id: TaskId, now: Timestamp) -> Result<(), StoreError> {
        let sql = "UPDATE tasks SET archived_at_ms = COALESCE(archived_at_ms, ?2) WHERE id = ?1";
        self.update_task(id, sql, now.as_millis())
    }

    /// Brings an archived task back to the lists.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn unarchive_task(&mut self, id: TaskId) -> Result<(), StoreError> {
        let sql = "UPDATE tasks SET archived_at_ms = NULL WHERE id = ?1";
        let changed = self.conn.execute(sql, [id.0])?;
        ensure_found(changed, "task", id.0)
    }

    /// Moves a task to the end of another project (`None` = no project).
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if the task does not exist; [`StoreError::Sqlite`] if the
    /// project does not exist.
    pub fn set_task_project(
        &mut self,
        id: TaskId,
        project: Option<ProjectId>,
    ) -> Result<(), StoreError> {
        let sql = "UPDATE tasks SET project_id = ?2, position = (
                       SELECT COALESCE(MAX(position), 0) + 1 FROM tasks WHERE project_id IS ?2
                   ) WHERE id = ?1";
        self.update_task(id, sql, project.map(|p| p.0))
    }

    /// Deletes a task. Its sessions stay as "No task"; its subtasks and their events are deleted.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn delete_task(&mut self, id: TaskId) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM tasks WHERE id = ?1", [id.0])?;
        ensure_found(changed, "task", id.0)
    }

    /// Runs `sql` with `?1` = the task and `?2` = `value`.
    fn update_task(
        &mut self,
        id: TaskId,
        sql: &str,
        value: impl rusqlite::ToSql,
    ) -> Result<(), StoreError> {
        let changed = self.conn.execute(sql, params![id.0, value])?;
        ensure_found(changed, "task", id.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::at;

    #[test]
    fn create_and_read_back() {
        let mut store = Store::open_in_memory().unwrap();
        let project = store.create_project("work", "#000000", at(0)).unwrap();
        let id = store.create_task(Some(project), "report", at(10)).unwrap();
        assert_eq!(
            store.task(id).unwrap(),
            Task {
                id,
                project: Some(project),
                name: "report".into(),
                completed_at: None,
                archived_at: None,
                position: 1,
                created_at: at(10),
            }
        );
    }

    #[test]
    fn position_is_appended_per_project() {
        let mut store = Store::open_in_memory().unwrap();
        let work = store.create_project("work", "#000000", at(0)).unwrap();
        let a = store.create_task(Some(work), "a", at(0)).unwrap();
        let b = store.create_task(Some(work), "b", at(0)).unwrap();
        let loose = store.create_task(None, "loose", at(0)).unwrap();
        assert_eq!(store.task(a).unwrap().position, 1);
        assert_eq!(store.task(b).unwrap().position, 2);
        assert_eq!(store.task(loose).unwrap().position, 1);
    }

    #[test]
    fn complete_and_reopen() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_task(None, "report", at(0)).unwrap();
        store.complete_task(id, at(30)).unwrap();
        assert_eq!(store.task(id).unwrap().completed_at, Some(at(30)));
        store.reopen_task(id).unwrap();
        assert_eq!(store.task(id).unwrap().completed_at, None);
    }

    #[test]
    fn rename_archive_and_unarchive() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_task(None, "report", at(0)).unwrap();
        store.rename_task(id, "summary").unwrap();
        store.archive_task(id, at(7)).unwrap();
        let task = store.task(id).unwrap();
        assert_eq!(task.name, "summary");
        assert_eq!(task.archived_at, Some(at(7)));
        store.unarchive_task(id).unwrap();
        assert_eq!(store.task(id).unwrap().archived_at, None);
    }

    #[test]
    fn moving_to_another_project_appends_at_the_end() {
        let mut store = Store::open_in_memory().unwrap();
        let work = store.create_project("work", "#000000", at(0)).unwrap();
        store.create_task(Some(work), "existing", at(0)).unwrap();
        let id = store.create_task(None, "report", at(0)).unwrap();
        store.set_task_project(id, Some(work)).unwrap();
        let task = store.task(id).unwrap();
        assert_eq!(task.project, Some(work));
        assert_eq!(task.position, 2);
        store.set_task_project(id, None).unwrap();
        assert_eq!(store.task(id).unwrap().project, None);
    }

    #[test]
    fn task_in_a_missing_project_is_rejected_by_the_foreign_key() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(matches!(
            store.create_task(Some(ProjectId(99)), "orphan", at(0)),
            Err(StoreError::Sqlite(_))
        ));
        let id = store.create_task(None, "report", at(0)).unwrap();
        assert!(store.set_task_project(id, Some(ProjectId(99))).is_err());
    }

    #[test]
    fn deleting_a_project_keeps_its_tasks_without_project() {
        let mut store = Store::open_in_memory().unwrap();
        let work = store.create_project("work", "#000000", at(0)).unwrap();
        let id = store.create_task(Some(work), "report", at(0)).unwrap();
        store.delete_project(work).unwrap();
        assert_eq!(store.task(id).unwrap().project, None);
    }

    #[test]
    fn deleting_a_project_appends_its_tasks_after_the_loose_ones() {
        let mut store = Store::open_in_memory().unwrap();
        let l1 = store.create_task(None, "l1", at(0)).unwrap();
        let l2 = store.create_task(None, "l2", at(0)).unwrap();
        let work = store.create_project("work", "#000000", at(0)).unwrap();
        let a = store.create_task(Some(work), "a", at(0)).unwrap();
        let b = store.create_task(Some(work), "b", at(0)).unwrap();
        store.delete_project(work).unwrap();
        let positions: Vec<_> = [l1, l2, a, b]
            .into_iter()
            .map(|id| store.task(id).unwrap().position)
            .collect();
        assert_eq!(positions, vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_failing_project_delete_keeps_task_positions() {
        let mut store = Store::open_in_memory().unwrap();
        store.create_task(None, "loose", at(0)).unwrap();
        let work = store.create_project("work", "#000000", at(0)).unwrap();
        let a = store.create_task(Some(work), "a", at(0)).unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER fail_delete BEFORE DELETE ON projects
                 BEGIN SELECT RAISE(ABORT, 'boom'); END;",
            )
            .unwrap();
        assert!(matches!(
            store.delete_project(work),
            Err(StoreError::Sqlite(_))
        ));
        assert_eq!(store.task(a).unwrap().position, 1);
        assert_eq!(store.task(a).unwrap().project, Some(work));
    }

    #[test]
    fn completing_or_archiving_again_keeps_the_first_time() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_task(None, "report", at(0)).unwrap();
        store.complete_task(id, at(10)).unwrap();
        store.complete_task(id, at(20)).unwrap();
        store.archive_task(id, at(30)).unwrap();
        store.archive_task(id, at(40)).unwrap();
        let task = store.task(id).unwrap();
        assert_eq!(task.completed_at, Some(at(10)));
        assert_eq!(task.archived_at, Some(at(30)));
    }

    #[test]
    fn delete_and_missing_tasks() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_task(None, "report", at(0)).unwrap();
        store.delete_task(id).unwrap();
        assert!(matches!(
            store.task(id),
            Err(StoreError::NotFound { entity: "task", .. })
        ));
        assert!(store.rename_task(id, "x").is_err());
        assert!(store.complete_task(id, at(0)).is_err());
        assert!(store.reopen_task(id).is_err());
        assert!(store.archive_task(id, at(0)).is_err());
        assert!(store.unarchive_task(id).is_err());
        assert!(store.set_task_project(id, None).is_err());
        assert!(store.delete_task(id).is_err());
    }
}
