//! The app's handle to the core. Every window goes through [`AppCore`], which owns the
//! [`Tracker`] (and through it the [`Store`]) and the clock: the timer panels use
//! [`AppCore::tracker`] / [`AppCore::tracker_mut`], the manage window and the picker the calls
//! below. One connection, so a task deleted in Gerenciar is also dropped from the timers.
//!
//! Lives on the UI thread, shared as `Rc<RefCell<AppCore>>`. Callers take short borrows: one
//! mutation, or one read that copies what the UI needs, never a borrow held across a UI call.

use std::path::Path;
use std::time::Duration;

use clocky_core::clock::{Clock, SystemClock};
use clocky_core::domain::{ProjectId, SubtaskId, TaskId, TimerId};
use clocky_core::engine::session::CompletedSession;
use clocky_core::engine::timers::ActiveTimer;
use clocky_core::store::{
    NewPomodoroPreset, Project, ProjectOverview, Session, Store, StoreError, Subtask, Task,
    TaskLinks, TaskSummary,
};
use clocky_core::time::Timestamp;
use clocky_core::tracker::{Tracker, TrackerError, TrackerEvent, TrackerOpenError};

/// Result of a core call. Store errors convert into it, so `?` works on both.
pub type CoreResult<T> = Result<T, TrackerError>;

pub struct AppCore {
    tracker: Tracker,
    clock: Box<dyn Clock>,
}

/// What the reopen tick did, for the timer panels.
#[derive(Debug, Default)]
pub struct Reopened {
    /// Events of the reopen tick (what happened while the app was closed).
    pub events: Vec<TrackerEvent>,
    /// The saved timers before that tick: a countdown that came due leaves the set, and the
    /// app still needs its name and task to say so.
    pub before: Vec<ActiveTimer>,
}

/// Why [`AppCore::open`] failed.
#[derive(Debug)]
pub enum OpenError {
    /// The database itself could not be opened (I/O, newer schema, not a database).
    Store(StoreError),
    /// The database opened but the active timers could not be restored; it holds the store.
    Timers(TrackerOpenError),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Timers(e) => write!(f, "{e}: {}", e.error),
        }
    }
}

impl From<StoreError> for OpenError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<TrackerOpenError> for OpenError {
    fn from(e: TrackerOpenError) -> Self {
        Self::Timers(e)
    }
}

impl AppCore {
    /// Opens (or creates) the database at `path` with the system clock and restores the timers.
    ///
    /// # Errors
    /// [`OpenError`].
    pub fn open(path: &Path) -> Result<(Self, Reopened), OpenError> {
        Ok(Self::with_store(Store::open(path)?, Box::new(SystemClock))?)
    }

    /// Restores the active timers of `store` and applies the reopen rules at `clock.now()`.
    ///
    /// # Errors
    /// [`TrackerOpenError`] if a saved timer cannot be restored or the database fails; it gives
    /// the store back (see [`TrackerOpenError::discard_active_timers`]).
    pub fn with_store(
        store: Store,
        clock: Box<dyn Clock>,
    ) -> Result<(Self, Reopened), TrackerOpenError> {
        // A corrupt timer makes this fail too; the tracker reports it right below.
        let before = store
            .load_active()
            .map(|(_, saved)| saved)
            .unwrap_or_default();
        let (tracker, events) = Tracker::open(store, clock.now())?;
        Ok((Self { tracker, clock }, Reopened { events, before }))
    }

    /// An empty in-memory database (screenshot runs without `CLOCKY_DB`).
    ///
    /// # Errors
    /// [`OpenError`] if SQLite fails.
    pub fn open_in_memory(clock: Box<dyn Clock>) -> Result<(Self, Reopened), OpenError> {
        Ok(Self::with_store(Store::open_in_memory()?, clock)?)
    }

    /// An empty in-memory core (tests).
    #[cfg(test)]
    pub fn in_memory(clock: Box<dyn Clock>) -> Self {
        Self::open_in_memory(clock).expect("in-memory database").0
    }

    /// The current instant, from the core's clock (the app never reads the clock elsewhere).
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn tracker(&self) -> &Tracker {
        &self.tracker
    }

    pub fn tracker_mut(&mut self) -> &mut Tracker {
        &mut self.tracker
    }

    fn store(&self) -> &Store {
        self.tracker.store()
    }

    fn store_mut(&mut self) -> &mut Store {
        self.tracker.store_mut()
    }
}

/// Reads.
impl AppCore {
    /// Active projects with their open and completed counts, in creation order.
    pub fn projects(&self) -> CoreResult<Vec<ProjectOverview>> {
        Ok(self.store().projects_overview()?)
    }

    pub fn project(&self, id: ProjectId) -> CoreResult<Project> {
        Ok(self.store().project(id)?)
    }

    /// Task cards of `project` (`None` = tasks without a project).
    pub fn task_summaries(
        &self,
        project: Option<ProjectId>,
        include_completed: bool,
    ) -> CoreResult<Vec<TaskSummary>> {
        Ok(self.store().task_summaries(project, include_completed)?)
    }

    pub fn task(&self, id: TaskId) -> CoreResult<Task> {
        Ok(self.store().task(id)?)
    }

    pub fn subtasks(&self, task: TaskId) -> CoreResult<Vec<Subtask>> {
        Ok(self.store().subtasks(task)?)
    }

    pub fn task_links(&self, task: TaskId) -> CoreResult<TaskLinks> {
        Ok(self.store().task_links(task)?)
    }

    pub fn task_time(&self, task: TaskId) -> CoreResult<Duration> {
        Ok(self.store().task_time(task)?)
    }

    pub fn recent_sessions(&self, task: TaskId, limit: u32) -> CoreResult<Vec<Session>> {
        Ok(self.store().recent_sessions(task, limit)?)
    }

    /// Tasks with the latest sessions ("Recentes" in the picker).
    pub fn recent_tasks(&self, limit: u32) -> CoreResult<Vec<Task>> {
        Ok(self.store().recent_tasks(limit)?)
    }

    /// Tasks whose name or project name contains `text`, ignoring case and accents.
    pub fn search_tasks(&self, text: &str) -> CoreResult<Vec<Task>> {
        Ok(self.store().search_tasks(text)?)
    }
}

/// Project changes.
impl AppCore {
    pub fn create_project(&mut self, name: &str, color: &str) -> CoreResult<ProjectId> {
        let now = self.now();
        Ok(self.store_mut().create_project(name, color, now)?)
    }

    /// Renames the project and changes its color (the edit form has both), all or nothing: if
    /// the color cannot be saved, the old name is put back.
    pub fn update_project(&mut self, id: ProjectId, name: &str, color: &str) -> CoreResult<()> {
        let old = self.store().project(id)?;
        self.store_mut().rename_project(id, name)?;
        if let Err(e) = self.store_mut().set_project_color(id, color) {
            if let Err(undo) = self.store_mut().rename_project(id, &old.name) {
                tracing::error!("failed to restore the name of project {}: {undo}", id.0);
            }
            return Err(e.into());
        }
        Ok(())
    }

    /// Whether the database has any project, archived ones included.
    pub fn any_project(&self) -> CoreResult<bool> {
        Ok(!self.store().projects(true)?.is_empty())
    }

    /// Hides the project from every list (its history stays in the reports).
    pub fn archive_project(&mut self, id: ProjectId) -> CoreResult<()> {
        let now = self.now();
        Ok(self.store_mut().archive_project(id, now)?)
    }

    /// Deletes the project; its tasks stay, without a project.
    pub fn delete_project(&mut self, id: ProjectId) -> CoreResult<()> {
        Ok(self.store_mut().delete_project(id)?)
    }
}

/// Task changes.
impl AppCore {
    pub fn create_task(&mut self, project: Option<ProjectId>, name: &str) -> CoreResult<TaskId> {
        let now = self.now();
        Ok(self.store_mut().create_task(project, name, now)?)
    }

    pub fn rename_task(&mut self, id: TaskId, name: &str) -> CoreResult<()> {
        Ok(self.store_mut().rename_task(id, name)?)
    }

    /// Completes (`done`) or reopens a task. Its subtasks are left as they are (spec, section 3).
    pub fn set_task_done(&mut self, id: TaskId, done: bool) -> CoreResult<()> {
        if done {
            let now = self.now();
            Ok(self.store_mut().complete_task(id, now)?)
        } else {
            Ok(self.store_mut().reopen_task(id)?)
        }
    }

    /// Moves a task to the end of `project` (`None` = no project).
    pub fn move_task(&mut self, id: TaskId, project: Option<ProjectId>) -> CoreResult<()> {
        Ok(self.store_mut().set_task_project(id, project)?)
    }

    pub fn archive_task(&mut self, id: TaskId) -> CoreResult<()> {
        let now = self.now();
        Ok(self.store_mut().archive_task(id, now)?)
    }

    /// Deletes a task and its subtasks; its sessions stay as "Sem tarefa". A running timer of
    /// that task goes on without one (the tracker drops the link).
    pub fn delete_task(&mut self, id: TaskId) -> CoreResult<()> {
        self.tracker.delete_task(id)
    }
}

/// Subtask changes (outside a timer: `run = None`).
impl AppCore {
    pub fn add_subtask(&mut self, task: TaskId, name: &str) -> CoreResult<SubtaskId> {
        Ok(self.store_mut().add_subtask(task, name)?)
    }

    pub fn rename_subtask(&mut self, id: SubtaskId, name: &str) -> CoreResult<()> {
        Ok(self.store_mut().rename_subtask(id, name)?)
    }

    /// Checks (`done`) or unchecks a subtask from the manage window.
    pub fn set_subtask_done(&mut self, id: SubtaskId, done: bool) -> CoreResult<()> {
        let now = self.now();
        if done {
            Ok(self.store_mut().complete_subtask(id, now, None)?)
        } else {
            Ok(self.store_mut().reopen_subtask(id, now, None)?)
        }
    }

    pub fn delete_subtask(&mut self, id: SubtaskId) -> CoreResult<()> {
        Ok(self.store_mut().delete_subtask(id)?)
    }
}

/// Writes used only by the demo data of the screenshot hook (real runs are recorded by the
/// tracker).
impl AppCore {
    pub fn create_timer_preset(
        &mut self,
        name: &str,
        duration: Duration,
        task: Option<TaskId>,
    ) -> CoreResult<()> {
        self.store_mut().create_timer_preset(name, duration, task)?;
        Ok(())
    }

    pub fn create_pomodoro_preset(&mut self, preset: &NewPomodoroPreset<'_>) -> CoreResult<()> {
        Ok(self.store_mut().create_pomodoro_preset(preset).map(drop)?)
    }

    pub fn record_session(
        &mut self,
        task: Option<TaskId>,
        run: TimerId,
        session: &CompletedSession,
    ) -> CoreResult<()> {
        self.store_mut().record_session(task, run, None, session)?;
        Ok(())
    }
}

// M7b: reports, the settings table and session deletion.
mod reports;

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
        let (mut core, reopened) = AppCore::open(&path).unwrap();
        assert!(reopened.events.is_empty() && reopened.before.is_empty());
        core.create_project("connect", "#3d8fd6").unwrap();
        let now = core.now();
        core.tracker_mut().start_stopwatch(None, now).unwrap();
        drop(core);
        let (core, reopened) = AppCore::open(&path).unwrap();
        assert_eq!(core.projects().unwrap().len(), 1);
        // The running stopwatch survives the reopen.
        assert_eq!(core.tracker().timers().iter().count(), 1);
        assert_eq!(reopened.before.len(), 1);
    }

    #[test]
    fn opening_a_directory_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            AppCore::open(dir.path()),
            Err(OpenError::Store(StoreError::Sqlite(_)))
        ));
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
        let now = core.now();
        let timer = core.tracker_mut().start_stopwatch(Some(t), now).unwrap();
        core.delete_task(t).unwrap();
        assert!(core.task(t).is_err());
        // The running stopwatch lost its task with it.
        assert_eq!(core.tracker().timers().get(timer).unwrap().task, None);
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
