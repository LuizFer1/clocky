//! Projects: create, rename, recolor, archive and delete.

use rusqlite::{Row, params};

use super::rows::{collect, ensure_found, first};
use super::{Store, StoreError};
use crate::domain::ProjectId;
use crate::time::Timestamp;

/// A project, which groups tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Identifier.
    pub id: ProjectId,
    /// Name shown in the lists.
    pub name: String,
    /// Color, as chosen in the UI (for example `#7c5cff`).
    pub color: String,
    /// When it was archived; `None` while active.
    pub archived_at: Option<Timestamp>,
    /// When it was created.
    pub created_at: Timestamp,
}

/// Columns read by [`project_from_row`], for a `projects` table aliased as `p`.
pub(super) const PROJECT_COLUMNS: &str = "p.id, p.name, p.color, p.archived_at_ms, p.created_at_ms";

/// Reads the [`PROJECT_COLUMNS`] at the start of `row`.
pub(super) fn project_from_row(row: &Row<'_>) -> Result<Project, StoreError> {
    Ok(Project {
        id: ProjectId(row.get(0)?),
        name: row.get(1)?,
        color: row.get(2)?,
        archived_at: row.get::<_, Option<i64>>(3)?.map(Timestamp::from_millis),
        created_at: Timestamp::from_millis(row.get(4)?),
    })
}

impl Store {
    /// Creates a project.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn create_project(
        &mut self,
        name: &str,
        color: &str,
        now: Timestamp,
    ) -> Result<ProjectId, StoreError> {
        self.conn.execute(
            "INSERT INTO projects (name, color, created_at_ms) VALUES (?1, ?2, ?3)",
            params![name, color, now.as_millis()],
        )?;
        Ok(ProjectId(self.conn.last_insert_rowid()))
    }

    /// The project with this identifier.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn project(&self, id: ProjectId) -> Result<Project, StoreError> {
        let sql = format!("SELECT {PROJECT_COLUMNS} FROM projects p WHERE p.id = ?1");
        first(
            collect(&self.conn, &sql, [id.0], project_from_row)?,
            "project",
            id.0,
        )
    }

    /// Projects in creation order; archived ones only if `include_archived`.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn projects(&self, include_archived: bool) -> Result<Vec<Project>, StoreError> {
        let sql = format!(
            "SELECT {PROJECT_COLUMNS} FROM projects p
             WHERE ?1 OR p.archived_at_ms IS NULL
             ORDER BY p.created_at_ms, p.id"
        );
        collect(&self.conn, &sql, [include_archived], project_from_row)
    }

    /// Renames a project.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn rename_project(&mut self, id: ProjectId, name: &str) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE projects SET name = ?1 WHERE id = ?2",
            params![name, id.0],
        )?;
        ensure_found(changed, "project", id.0)
    }

    /// Changes the color of a project.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn set_project_color(&mut self, id: ProjectId, color: &str) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE projects SET color = ?1 WHERE id = ?2",
            params![color, id.0],
        )?;
        ensure_found(changed, "project", id.0)
    }

    /// Archives a project: it leaves the lists, and its history stays in the reports. If it is
    /// already archived, the original time is kept.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn archive_project(&mut self, id: ProjectId, now: Timestamp) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE projects SET archived_at_ms = COALESCE(archived_at_ms, ?1) WHERE id = ?2",
            params![now.as_millis(), id.0],
        )?;
        ensure_found(changed, "project", id.0)
    }

    /// Brings an archived project back to the lists.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn unarchive_project(&mut self, id: ProjectId) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE projects SET archived_at_ms = NULL WHERE id = ?1",
            [id.0],
        )?;
        ensure_found(changed, "project", id.0)
    }

    /// Deletes a project. Its tasks stay, without a project, after the tasks that already had
    /// none (in their previous order).
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if it does not exist.
    pub fn delete_project(&mut self, id: ProjectId) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE tasks SET position = position + (
                 SELECT COALESCE(MAX(position), 0) FROM tasks WHERE project_id IS NULL
             ) WHERE project_id = ?1",
            [id.0],
        )?;
        let changed = tx.execute("DELETE FROM projects WHERE id = ?1", [id.0])?;
        ensure_found(changed, "project", id.0)?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::at;

    #[test]
    fn create_and_read_back() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_project("work", "#7c5cff", at(10)).unwrap();
        assert_eq!(
            store.project(id).unwrap(),
            Project {
                id,
                name: "work".into(),
                color: "#7c5cff".into(),
                archived_at: None,
                created_at: at(10),
            }
        );
    }

    #[test]
    fn rename_and_recolor() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_project("work", "#000000", at(0)).unwrap();
        store.rename_project(id, "job").unwrap();
        store.set_project_color(id, "#ffffff").unwrap();
        let project = store.project(id).unwrap();
        assert_eq!(project.name, "job");
        assert_eq!(project.color, "#ffffff");
    }

    #[test]
    fn archived_projects_leave_the_default_list() {
        let mut store = Store::open_in_memory().unwrap();
        let work = store.create_project("work", "#000000", at(0)).unwrap();
        let home = store.create_project("home", "#000000", at(1)).unwrap();
        store.archive_project(work, at(5)).unwrap();

        let active: Vec<_> = store
            .projects(false)
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(active, vec![home]);
        let all: Vec<_> = store.projects(true).unwrap().iter().map(|p| p.id).collect();
        assert_eq!(all, vec![work, home]);
        assert_eq!(store.project(work).unwrap().archived_at, Some(at(5)));

        store.unarchive_project(work).unwrap();
        assert_eq!(store.projects(false).unwrap().len(), 2);
    }

    #[test]
    fn archiving_again_keeps_the_first_time() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_project("work", "#000000", at(0)).unwrap();
        store.archive_project(id, at(5)).unwrap();
        store.archive_project(id, at(9)).unwrap();
        assert_eq!(store.project(id).unwrap().archived_at, Some(at(5)));
    }

    #[test]
    fn delete_removes_the_project() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_project("work", "#000000", at(0)).unwrap();
        store.delete_project(id).unwrap();
        assert!(matches!(
            store.project(id),
            Err(StoreError::NotFound {
                entity: "project",
                ..
            })
        ));
    }

    #[test]
    fn changing_a_missing_project_is_not_found() {
        let mut store = Store::open_in_memory().unwrap();
        let missing = ProjectId(99);
        assert!(matches!(
            store.rename_project(missing, "x"),
            Err(StoreError::NotFound { id: 99, .. })
        ));
        assert!(store.set_project_color(missing, "#000000").is_err());
        assert!(store.archive_project(missing, at(0)).is_err());
        assert!(store.unarchive_project(missing).is_err());
        assert!(store.delete_project(missing).is_err());
    }
}
