//! Subtasks (one level, guaranteed by the schema) and their completion events.

use std::time::Duration;

use rusqlite::{OptionalExtension, Row, params};

use super::convert::{estimate_from_sql, estimate_to_sql, timer_id_from_sql, timer_id_to_sql};
use super::rows::{collect, ensure_found};
use super::{Store, StoreError};
use crate::domain::{SessionId, SubtaskId, TaskId, TimerId};
use crate::time::Timestamp;

/// A checklist item of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtask {
    /// Identifier.
    pub id: SubtaskId,
    /// Parent task.
    pub task: TaskId,
    /// Name.
    pub name: String,
    /// Free text about the subtask; empty = none.
    pub description: String,
    /// How long the subtask is expected to take; `None` = no estimate.
    pub estimate: Option<Duration>,
    /// When it was completed; `None` while open.
    pub completed_at: Option<Timestamp>,
    /// Order inside the task (ascending).
    pub position: i64,
}

/// What happened to a subtask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtaskEventKind {
    /// It was checked.
    Completed,
    /// It was unchecked.
    Reopened,
}

impl SubtaskEventKind {
    fn to_sql(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Reopened => "reopened",
        }
    }

    fn from_sql(text: String) -> Result<Self, StoreError> {
        match text.as_str() {
            "completed" => Ok(Self::Completed),
            "reopened" => Ok(Self::Reopened),
            _ => Err(StoreError::Corrupt {
                what: "subtask event kind",
                value: text,
            }),
        }
    }
}

/// A subtask was checked or unchecked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskEvent {
    /// The subtask.
    pub subtask: SubtaskId,
    /// Checked or unchecked.
    pub kind: SubtaskEventKind,
    /// When.
    pub at: Timestamp,
    /// Timer whose checklist was clicked; `None` = outside a timer.
    pub run: Option<TimerId>,
    /// Session of that timer that contains the event, once it is recorded.
    pub session: Option<SessionId>,
}

/// Columns read by [`event_from_row`], for a `subtask_events` table aliased as `e`.
pub(super) const EVENT_COLUMNS: &str = "e.subtask_id, e.kind, e.at_ms, e.run_id, e.session_id";

/// Reads the [`EVENT_COLUMNS`] at the start of `row`.
pub(super) fn event_from_row(row: &Row<'_>) -> Result<SubtaskEvent, StoreError> {
    Ok(SubtaskEvent {
        subtask: SubtaskId(row.get(0)?),
        kind: SubtaskEventKind::from_sql(row.get(1)?)?,
        at: Timestamp::from_millis(row.get(2)?),
        run: row
            .get::<_, Option<i64>>(3)?
            .map(timer_id_from_sql)
            .transpose()?,
        session: row.get::<_, Option<i64>>(4)?.map(SessionId),
    })
}

fn subtask_from_row(row: &Row<'_>) -> Result<Subtask, StoreError> {
    Ok(Subtask {
        id: SubtaskId(row.get(0)?),
        task: TaskId(row.get(1)?),
        name: row.get(2)?,
        completed_at: row.get::<_, Option<i64>>(3)?.map(Timestamp::from_millis),
        position: row.get(4)?,
        description: row.get(5)?,
        estimate: estimate_from_sql(row.get(6)?)?,
    })
}

impl Store {
    /// Adds a subtask at the end of `task`.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if the task does not exist or SQLite fails.
    pub fn add_subtask(&mut self, task: TaskId, name: &str) -> Result<SubtaskId, StoreError> {
        self.conn.execute(
            "INSERT INTO subtasks (task_id, name, position)
             SELECT ?1, ?2, COALESCE(MAX(position), 0) + 1 FROM subtasks WHERE task_id = ?1",
            params![task.0, name],
        )?;
        Ok(SubtaskId(self.conn.last_insert_rowid()))
    }

    /// Subtasks of `task`, in order.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn subtasks(&self, task: TaskId) -> Result<Vec<Subtask>, StoreError> {
        let sql = "SELECT id, task_id, name, completed_at_ms, position, description, estimate_ms FROM subtasks
                   WHERE task_id = ?1 ORDER BY position, id";
        collect(&self.conn, sql, [task.0], subtask_from_row)
    }

    /// Renames a subtask.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn rename_subtask(&mut self, id: SubtaskId, name: &str) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE subtasks SET name = ?1 WHERE id = ?2",
            params![name, id.0],
        )?;
        ensure_found(changed, "subtask", id.0)
    }

    /// Changes the description of a subtask (`""` clears it).
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn set_subtask_description(
        &mut self,
        id: SubtaskId,
        description: &str,
    ) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE subtasks SET description = ?1 WHERE id = ?2",
            params![description, id.0],
        )?;
        ensure_found(changed, "subtask", id.0)
    }

    /// Changes the estimated time of a subtask (`None` clears it).
    ///
    /// # Errors
    /// [`StoreError::OutOfRange`] if `estimate` is zero or too long; [`StoreError::NotFound`] if
    /// the subtask does not exist.
    pub fn set_subtask_estimate(
        &mut self,
        id: SubtaskId,
        estimate: Option<Duration>,
    ) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE subtasks SET estimate_ms = ?1 WHERE id = ?2",
            params![estimate_to_sql(estimate)?, id.0],
        )?;
        ensure_found(changed, "subtask", id.0)
    }

    /// Deletes a subtask and its events.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn delete_subtask(&mut self, id: SubtaskId) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM subtasks WHERE id = ?1", [id.0])?;
        ensure_found(changed, "subtask", id.0)
    }

    /// Checks a subtask at `at`, clicked in the checklist of timer `run` (`None` = outside a
    /// timer). Does nothing if it is already checked.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist; [`StoreError::OutOfRange`] if `run` does
    /// not fit an `INTEGER`.
    pub fn complete_subtask(
        &mut self,
        id: SubtaskId,
        at: Timestamp,
        run: Option<TimerId>,
    ) -> Result<(), StoreError> {
        self.change_subtask(id, SubtaskEventKind::Completed, at, run)
    }

    /// Unchecks a subtask at `at` (same rules as [`Store::complete_subtask`]).
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist; [`StoreError::OutOfRange`] if `run` does
    /// not fit an `INTEGER`.
    pub fn reopen_subtask(
        &mut self,
        id: SubtaskId,
        at: Timestamp,
        run: Option<TimerId>,
    ) -> Result<(), StoreError> {
        self.change_subtask(id, SubtaskEventKind::Reopened, at, run)
    }

    /// Events of a subtask, oldest first.
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] or [`StoreError::OutOfRange`] if a stored value is invalid.
    pub fn subtask_events(&self, id: SubtaskId) -> Result<Vec<SubtaskEvent>, StoreError> {
        let sql = format!(
            "SELECT {EVENT_COLUMNS} FROM subtask_events e
             WHERE e.subtask_id = ?1 ORDER BY e.at_ms, e.id"
        );
        collect(&self.conn, &sql, [id.0], event_from_row)
    }

    /// Updates `completed_at_ms` and records the event, in one transaction.
    fn change_subtask(
        &mut self,
        id: SubtaskId,
        kind: SubtaskEventKind,
        at: Timestamp,
        run: Option<TimerId>,
    ) -> Result<(), StoreError> {
        let run_id = run.map(timer_id_to_sql).transpose()?;
        let completed_at = (kind == SubtaskEventKind::Completed).then_some(at.as_millis());
        let tx = self.conn.transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT completed_at_ms FROM subtasks WHERE id = ?1",
                [id.0],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound {
                entity: "subtask",
                id: id.0,
            })?;
        if current.is_some() == completed_at.is_some() {
            return Ok(());
        }
        tx.execute(
            "UPDATE subtasks SET completed_at_ms = ?1 WHERE id = ?2",
            params![completed_at, id.0],
        )?;
        tx.execute(
            "INSERT INTO subtask_events (subtask_id, kind, at_ms, run_id) VALUES (?1, ?2, ?3, ?4)",
            params![id.0, kind.to_sql(), at.as_millis(), run_id],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::at;

    fn store_with_task() -> (Store, TaskId) {
        let mut store = Store::open_in_memory().unwrap();
        let task = store.create_task(None, "report", at(0)).unwrap();
        (store, task)
    }

    #[test]
    fn description_and_estimate_are_saved_and_cleared() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        let fresh = &store.subtasks(task).unwrap()[0];
        assert_eq!((fresh.description.as_str(), fresh.estimate), ("", None));
        let half_hour = Duration::from_mins(30);
        store.set_subtask_description(id, "Three sections").unwrap();
        store.set_subtask_estimate(id, Some(half_hour)).unwrap();
        let saved = &store.subtasks(task).unwrap()[0];
        assert_eq!(saved.description, "Three sections");
        assert_eq!(saved.estimate, Some(half_hour));
        assert!(matches!(
            store.set_subtask_estimate(id, Some(Duration::ZERO)),
            Err(StoreError::OutOfRange(_))
        ));
        store.set_subtask_description(id, "").unwrap();
        store.set_subtask_estimate(id, None).unwrap();
        let cleared = &store.subtasks(task).unwrap()[0];
        assert_eq!((cleared.description.as_str(), cleared.estimate), ("", None));
        let missing = SubtaskId(99);
        assert!(matches!(
            store.set_subtask_description(missing, "x"),
            Err(StoreError::NotFound { .. })
        ));
        assert!(matches!(
            store.set_subtask_estimate(missing, Some(half_hour)),
            Err(StoreError::NotFound { .. })
        ));
    }

    #[test]
    fn add_list_rename_and_delete() {
        let (mut store, task) = store_with_task();
        let a = store.add_subtask(task, "outline").unwrap();
        let b = store.add_subtask(task, "draft").unwrap();
        store.rename_subtask(a, "plan").unwrap();
        let names: Vec<_> = store
            .subtasks(task)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["plan", "draft"]);
        assert_eq!(store.subtasks(task).unwrap()[1].position, 2);
        store.delete_subtask(b).unwrap();
        assert_eq!(store.subtasks(task).unwrap().len(), 1);
    }

    #[test]
    fn subtasks_have_a_single_level() {
        let (store, _) = store_with_task();
        let parents: Vec<String> = store
            .conn
            .prepare("SELECT \"table\" FROM pragma_foreign_key_list('subtasks')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(parents, vec!["tasks"]);
    }

    #[test]
    fn complete_and_reopen_record_events() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        store
            .complete_subtask(id, at(10), Some(TimerId(3)))
            .unwrap();
        assert_eq!(store.subtasks(task).unwrap()[0].completed_at, Some(at(10)));
        store.reopen_subtask(id, at(20), None).unwrap();
        assert_eq!(store.subtasks(task).unwrap()[0].completed_at, None);
        assert_eq!(
            store.subtask_events(id).unwrap(),
            vec![
                SubtaskEvent {
                    subtask: id,
                    kind: SubtaskEventKind::Completed,
                    at: at(10),
                    run: Some(TimerId(3)),
                    session: None,
                },
                SubtaskEvent {
                    subtask: id,
                    kind: SubtaskEventKind::Reopened,
                    at: at(20),
                    run: None,
                    session: None,
                },
            ]
        );
    }

    #[test]
    fn completing_twice_records_one_event() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        store.complete_subtask(id, at(10), None).unwrap();
        store.complete_subtask(id, at(11), None).unwrap();
        store.reopen_subtask(id, at(12), None).unwrap();
        store.reopen_subtask(id, at(13), None).unwrap();
        assert_eq!(store.subtask_events(id).unwrap().len(), 2);
        assert_eq!(store.subtasks(task).unwrap()[0].completed_at, None);
    }

    #[test]
    fn completing_every_subtask_leaves_the_task_open() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        store.complete_subtask(id, at(10), None).unwrap();
        assert_eq!(store.task(task).unwrap().completed_at, None);
    }

    #[test]
    fn deleting_the_task_deletes_subtasks_and_events() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        store.complete_subtask(id, at(10), None).unwrap();
        store.delete_task(task).unwrap();
        assert_eq!(store.subtasks(task).unwrap().len(), 0);
        assert_eq!(store.subtask_events(id).unwrap().len(), 0);
    }

    #[test]
    fn subtask_of_a_missing_task_is_rejected() {
        let (mut store, _) = store_with_task();
        assert!(matches!(
            store.add_subtask(TaskId(99), "orphan"),
            Err(StoreError::Sqlite(_))
        ));
    }

    #[test]
    fn missing_subtasks_are_not_found() {
        let (mut store, _) = store_with_task();
        let missing = SubtaskId(99);
        assert!(matches!(
            store.complete_subtask(missing, at(0), None),
            Err(StoreError::NotFound {
                entity: "subtask",
                id: 99
            })
        ));
        assert!(store.reopen_subtask(missing, at(0), None).is_err());
        assert!(store.rename_subtask(missing, "x").is_err());
        assert!(store.delete_subtask(missing).is_err());
    }

    #[test]
    fn run_that_does_not_fit_is_rejected_without_changes() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        assert!(matches!(
            store.complete_subtask(id, at(0), Some(TimerId(u64::MAX))),
            Err(StoreError::OutOfRange("timer id"))
        ));
        assert_eq!(store.subtasks(task).unwrap()[0].completed_at, None);
    }

    #[test]
    fn a_failing_event_insert_leaves_the_subtask_unchanged() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER fail_events BEFORE INSERT ON subtask_events
                 BEGIN SELECT RAISE(ABORT, 'boom'); END;",
            )
            .unwrap();
        assert!(matches!(
            store.complete_subtask(id, at(10), None),
            Err(StoreError::Sqlite(_))
        ));
        assert_eq!(store.subtasks(task).unwrap()[0].completed_at, None);
        assert_eq!(store.subtask_events(id).unwrap().len(), 0);
    }

    #[test]
    fn corrupted_events_are_errors() {
        let (mut store, task) = store_with_task();
        let id = store.add_subtask(task, "outline").unwrap();
        store.complete_subtask(id, at(10), None).unwrap();
        store
            .conn
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 UPDATE subtask_events SET kind = 'bogus';",
            )
            .unwrap();
        assert!(matches!(
            store.subtask_events(id),
            Err(StoreError::Corrupt { .. })
        ));
        store
            .conn
            .execute_batch("UPDATE subtask_events SET kind = 'completed', run_id = -1;")
            .unwrap();
        assert!(matches!(
            store.subtask_events(id),
            Err(StoreError::OutOfRange("timer id"))
        ));
    }
}
