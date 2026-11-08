//! The app's handle to the core. Every window goes through [`AppCore`]; no other module names
//! [`Store`]. When M2b's `Tracker` lands, only this file changes: `AppCore` owns the tracker
//! and forwards these calls to `tracker.store()` / `store_mut()`.
//!
//! Lives on the UI thread, shared as `Rc<RefCell<AppCore>>`. Callers take short borrows: one
//! mutation, or one read that copies what the UI needs, never a borrow held across a UI call.

use std::path::Path;
use std::time::Duration;

use clocky_core::clock::{Clock, SystemClock};
use clocky_core::domain::{ProjectId, SubtaskId, TaskId, TimerId};
use clocky_core::engine::session::CompletedSession;
use clocky_core::store::{
    NewPomodoroPreset, Project, ProjectOverview, Session, Store, StoreError, Subtask, Task,
    TaskLinks, TaskSummary,
};
use clocky_core::time::Timestamp;

/// Result of a core call.
pub type CoreResult<T> = Result<T, StoreError>;

pub struct AppCore {
    store: Store,
    clock: Box<dyn Clock>,
}

impl AppCore {
    /// Opens (or creates) the database at `path` with the system clock.
    pub fn open(path: &Path) -> CoreResult<Self> {
        Ok(Self::new(Store::open(path)?, Box::new(SystemClock)))
    }

    pub fn new(store: Store, clock: Box<dyn Clock>) -> Self {
        Self { store, clock }
    }

    /// An empty in-memory core (tests).
    #[cfg(test)]
    pub fn in_memory(clock: Box<dyn Clock>) -> Self {
        let store = Store::open_in_memory().expect("in-memory database");
        Self::new(store, clock)
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }
}

/// Reads.
impl AppCore {
    /// Active projects with their open and completed counts, in creation order.
    pub fn projects(&self) -> CoreResult<Vec<ProjectOverview>> {
        self.store.projects_overview()
    }

    pub fn project(&self, id: ProjectId) -> CoreResult<Project> {
        self.store.project(id)
    }

    /// Task cards of `project` (`None` = tasks without a project).
    pub fn task_summaries(
        &self,
        project: Option<ProjectId>,
        include_completed: bool,
    ) -> CoreResult<Vec<TaskSummary>> {
        self.store.task_summaries(project, include_completed)
    }

    pub fn task(&self, id: TaskId) -> CoreResult<Task> {
        self.store.task(id)
    }

    pub fn subtasks(&self, task: TaskId) -> CoreResult<Vec<Subtask>> {
        self.store.subtasks(task)
    }

    pub fn task_links(&self, task: TaskId) -> CoreResult<TaskLinks> {
        self.store.task_links(task)
    }

    pub fn task_time(&self, task: TaskId) -> CoreResult<Duration> {
        self.store.task_time(task)
    }

    pub fn recent_sessions(&self, task: TaskId, limit: u32) -> CoreResult<Vec<Session>> {
        self.store.recent_sessions(task, limit)
    }

    /// Tasks with the latest sessions ("Recentes" in the picker).
    pub fn recent_tasks(&self, limit: u32) -> CoreResult<Vec<Task>> {
        self.store.recent_tasks(limit)
    }

    /// Tasks whose name or project name contains `text`, ignoring case and accents.
    pub fn search_tasks(&self, text: &str) -> CoreResult<Vec<Task>> {
        self.store.search_tasks(text)
    }
}

/// Project changes.
impl AppCore {
    pub fn create_project(&mut self, name: &str, color: &str) -> CoreResult<ProjectId> {
        let now = self.now();
        self.store.create_project(name, color, now)
    }

    /// Renames the project and changes its color (the edit form has both), all or nothing: if
    /// the color cannot be saved, the old name is put back.
    pub fn update_project(&mut self, id: ProjectId, name: &str, color: &str) -> CoreResult<()> {
        let old = self.store.project(id)?;
        self.store.rename_project(id, name)?;
        if let Err(e) = self.store.set_project_color(id, color) {
            if let Err(undo) = self.store.rename_project(id, &old.name) {
                tracing::error!("failed to restore the name of project {}: {undo}", id.0);
            }
            return Err(e);
        }
        Ok(())
    }

    /// Whether the database has any project, archived ones included.
    pub fn any_project(&self) -> CoreResult<bool> {
        Ok(!self.store.projects(true)?.is_empty())
    }

    /// Hides the project from every list (its history stays in the reports).
    pub fn archive_project(&mut self, id: ProjectId) -> CoreResult<()> {
        let now = self.now();
        self.store.archive_project(id, now)
    }

    /// Deletes the project; its tasks stay, without a project.
    pub fn delete_project(&mut self, id: ProjectId) -> CoreResult<()> {
        self.store.delete_project(id)
    }
}

/// Task changes.
impl AppCore {
    pub fn create_task(&mut self, project: Option<ProjectId>, name: &str) -> CoreResult<TaskId> {
        let now = self.now();
        self.store.create_task(project, name, now)
    }

    pub fn rename_task(&mut self, id: TaskId, name: &str) -> CoreResult<()> {
        self.store.rename_task(id, name)
    }

    /// Completes (`done`) or reopens a task. Its subtasks are left as they are (spec, section 3).
    pub fn set_task_done(&mut self, id: TaskId, done: bool) -> CoreResult<()> {
        if done {
            let now = self.now();
            self.store.complete_task(id, now)
        } else {
            self.store.reopen_task(id)
        }
    }

    /// Moves a task to the end of `project` (`None` = no project).
    pub fn move_task(&mut self, id: TaskId, project: Option<ProjectId>) -> CoreResult<()> {
        self.store.set_task_project(id, project)
    }

    pub fn archive_task(&mut self, id: TaskId) -> CoreResult<()> {
        let now = self.now();
        self.store.archive_task(id, now)
    }

    /// Deletes a task and its subtasks; its sessions stay as "Sem tarefa".
    pub fn delete_task(&mut self, id: TaskId) -> CoreResult<()> {
        self.store.delete_task(id)
    }
}

/// Subtask changes (outside a timer: `run = None`).
impl AppCore {
    pub fn add_subtask(&mut self, task: TaskId, name: &str) -> CoreResult<SubtaskId> {
        self.store.add_subtask(task, name)
    }

    pub fn rename_subtask(&mut self, id: SubtaskId, name: &str) -> CoreResult<()> {
        self.store.rename_subtask(id, name)
    }

    /// Checks (`done`) or unchecks a subtask from the manage window.
    pub fn set_subtask_done(&mut self, id: SubtaskId, done: bool) -> CoreResult<()> {
        let now = self.now();
        if done {
            self.store.complete_subtask(id, now, None)
        } else {
            self.store.reopen_subtask(id, now, None)
        }
    }

    pub fn delete_subtask(&mut self, id: SubtaskId) -> CoreResult<()> {
        self.store.delete_subtask(id)
    }
}

/// Writes used only by the demo data of the screenshot hook. M6b replaces the session one with
/// the tracker recording real runs.
impl AppCore {
    pub fn create_timer_preset(
        &mut self,
        name: &str,
        duration: Duration,
        task: Option<TaskId>,
    ) -> CoreResult<()> {
        self.store
            .create_timer_preset(name, duration, task)
            .map(drop)
    }

    pub fn create_pomodoro_preset(&mut self, preset: &NewPomodoroPreset<'_>) -> CoreResult<()> {
        self.store.create_pomodoro_preset(preset).map(drop)
    }

    pub fn record_session(
        &mut self,
        task: Option<TaskId>,
        run: TimerId,
        session: &CompletedSession,
    ) -> CoreResult<()> {
        self.store
            .record_session(task, run, None, session)
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use clocky_core::clock::FakeClock;

    use super::*;

    fn core() -> AppCore {
        AppCore::in_memory(Box::new(FakeClock::new(Timestamp::from_millis(1_000))))
    }

    #[test]
    fn opens_a_file_and_reopens_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clocky.db");
        let mut core = AppCore::open(&path).unwrap();
        core.create_project("connect", "#3d8fd6").unwrap();
        drop(core);
        let core = AppCore::open(&path).unwrap();
        assert_eq!(core.projects().unwrap().len(), 1);
    }

    #[test]
    fn opening_a_directory_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(AppCore::open(dir.path()).is_err());
    }

    #[test]
    fn project_edit_archive_and_delete() {
        let mut core = core();
        let id = core.create_project("a", "#3d8fd6").unwrap();
        core.update_project(id, "b", "#2fb3a6").unwrap();
        let project = core.project(id).unwrap();
        assert_eq!(
            (project.name.as_str(), project.color.as_str()),
            ("b", "#2fb3a6")
        );
        core.archive_project(id).unwrap();
        assert_eq!(core.projects().unwrap().len(), 0);
        core.delete_project(id).unwrap();
        assert!(core.project(id).is_err());
    }

    #[test]
    fn archived_projects_still_count_as_existing() {
        let mut core = core();
        assert!(!core.any_project().unwrap());
        let id = core.create_project("a", "#3d8fd6").unwrap();
        core.archive_project(id).unwrap();
        assert!(core.any_project().unwrap());
    }

    #[test]
    fn updating_a_missing_project_changes_nothing() {
        let mut core = core();
        assert!(core.update_project(ProjectId(42), "x", "#3d8fd6").is_err());
    }

    #[test]
    fn task_done_move_archive_and_delete() {
        let mut core = core();
        let p = core.create_project("a", "#3d8fd6").unwrap();
        let q = core.create_project("b", "#3d8fd6").unwrap();
        let t = core.create_task(Some(p), "x").unwrap();
        core.set_task_done(t, true).unwrap();
        assert!(core.task(t).unwrap().completed_at.is_some());
        core.set_task_done(t, false).unwrap();
        assert!(core.task(t).unwrap().completed_at.is_none());
        core.rename_task(t, "y").unwrap();
        core.move_task(t, Some(q)).unwrap();
        assert_eq!(core.task(t).unwrap().project, Some(q));
        core.archive_task(t).unwrap();
        assert_eq!(core.task_summaries(Some(q), true).unwrap().len(), 0);
        core.delete_task(t).unwrap();
        assert!(core.task(t).is_err());
    }

    #[test]
    fn subtasks_check_uncheck_rename_delete() {
        let mut core = core();
        let t = core.create_task(None, "x").unwrap();
        let s = core.add_subtask(t, "one").unwrap();
        core.set_subtask_done(s, true).unwrap();
        assert!(core.subtasks(t).unwrap()[0].completed_at.is_some());
        core.set_subtask_done(s, false).unwrap();
        assert!(core.subtasks(t).unwrap()[0].completed_at.is_none());
        core.rename_subtask(s, "two").unwrap();
        assert_eq!(core.subtasks(t).unwrap()[0].name, "two");
        core.delete_subtask(s).unwrap();
        assert_eq!(core.subtasks(t).unwrap().len(), 0);
    }
}
