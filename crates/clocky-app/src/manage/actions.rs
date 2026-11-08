//! What the manage window's callbacks do: navigation, then project, task and subtask changes.
// Slint callbacks hand their arguments over by value.
#![allow(clippy::needless_pass_by_value)]

use clocky_core::domain::{ProjectId, SubtaskId, TaskId};
use slint::SharedString;

use super::{Manage, Pending, Section, TimerStart, clean, db_id};
use crate::manage_view::Nav;
use crate::palette;

impl Manage {
    pub(super) fn connect(&self) {
        self.connect_navigation();
        self.connect_projects();
        self.connect_tasks();
        self.connect_subtasks();
        // M7b: Relatórios and Configurações.
        self.connect_reports();
        self.connect_settings();
    }

    fn connect_navigation(&self) {
        let state = self.state();
        state.on_show_all(self.handler0(Self::show_all));
        state.on_select_project(self.handler1(Self::select_project));
        state.on_open_task(self.handler1(Self::open_task));
        state.on_back_to_project(self.handler0(Self::back_to_project));
        state.on_search_edited(self.handler1(Self::search_edited));
        state.on_set_show_completed(self.handler1(Self::set_show_completed));
        state.on_confirm_accepted(self.handler0(Self::confirm_accepted));
        state.on_confirm_cancelled(self.handler0(Self::close_confirm));
    }

    fn connect_projects(&self) {
        let state = self.state();
        state.on_new_project(self.handler0(Self::new_project));
        state.on_edit_project(self.handler0(Self::edit_project));
        state.on_save_project(self.handler2(Self::save_project));
        state.on_archive_project(self.handler0(Self::ask_archive_project));
        state.on_delete_project(self.handler0(Self::ask_delete_project));
    }

    fn connect_tasks(&self) {
        let state = self.state();
        state.on_create_task(self.handler1(Self::create_task));
        state.on_set_task_done(self.handler2(Self::set_task_done));
        state.on_toggle_current_task(self.handler0(Self::toggle_current_task));
        state.on_move_to_no_project(self.handler0(|m| m.move_task_to(None)));
        state.on_rename_task(self.handler1(Self::rename_task));
        state.on_move_task(self.handler1(Self::move_task));
        state.on_archive_task(self.handler0(Self::ask_archive_task));
        state.on_delete_task(self.handler0(Self::ask_delete_task));
        state.on_start_pomodoro(self.handler0(|m| m.start_timer(TimerStart::Pomodoro)));
        state.on_start_stopwatch(self.handler0(|m| m.start_timer(TimerStart::Stopwatch)));
        state.on_link_task(self.handler0(|_| {
            tracing::info!("\"+ Vincular\" requested (linking presets and alarms comes later)");
        }));
    }

    fn connect_subtasks(&self) {
        let state = self.state();
        state.on_add_subtask(self.handler1(Self::add_subtask));
        state.on_set_subtask_done(self.handler2(Self::set_subtask_done));
        state.on_rename_subtask(self.handler2(Self::rename_subtask));
        state.on_delete_subtask(self.handler1(Self::delete_subtask));
    }
}

/// Navigation.
impl Manage {
    pub fn show_all(&self) {
        let nav = Nav {
            show_completed: false,
            ..Nav::default()
        };
        self.navigate(Nav {
            search: self.nav().search,
            ..nav
        });
    }

    pub fn select_project(&self, id: i32) {
        if let Some(id) = db_id(id) {
            self.go_to_project(ProjectId(id));
        }
    }

    fn go_to_project(&self, id: ProjectId) {
        self.navigate(Nav {
            project: Some(id),
            task: None,
            show_completed: false,
            search: self.nav().search,
        });
    }

    /// Forgets the project and task pages before they are deleted or archived, so the refresh
    /// that follows does not look for them.
    pub(super) fn leave(&self, project: bool) {
        let mut nav = self.nav.borrow_mut();
        nav.task = None;
        if project {
            nav.project = None;
        }
    }

    pub fn open_task(&self, id: i32) {
        if let Some(id) = db_id(id).map(TaskId) {
            self.open_task_id(id);
        }
    }

    pub(super) fn open_task_id(&self, id: TaskId) {
        let project = self.core.borrow().task(id).map(|t| t.project);
        match project {
            Ok(project) => self.navigate(Nav {
                project,
                task: Some(id),
                ..self.nav()
            }),
            Err(e) => tracing::warn!("cannot open task {}: {e}", id.0),
        }
    }

    pub(super) fn back_to_project(&self) {
        self.navigate(Nav {
            task: None,
            ..self.nav()
        });
    }

    fn search_edited(&self, text: SharedString) {
        self.section.set(Section::Projects);
        let mut nav = self.nav.borrow_mut();
        nav.search = text.to_string();
        // Searching leaves the task page for the matching list.
        nav.task = None;
        drop(nav);
        self.refresh();
    }

    fn set_show_completed(&self, show: bool) {
        self.nav.borrow_mut().show_completed = show;
        self.refresh();
    }

    fn start_timer(&self, kind: TimerStart) {
        let Some(task) = self.nav().task else { return };
        // Taken out while it runs, so a hook that touches the window cannot hit a held borrow.
        let hook = self.on_start_timer.borrow_mut().take();
        if let Some(hook) = hook {
            hook(kind, task);
            self.on_start_timer.borrow_mut().get_or_insert(hook);
        } else {
            tracing::info!("start {kind:?} for task {} (wired in M6b)", task.0);
        }
    }
}

/// Projects.
impl Manage {
    fn new_project(&self) {
        let nav = self.nav();
        self.navigate(Nav {
            project: None,
            task: None,
            ..nav
        });
        let state = self.state();
        state.set_project_form_name(SharedString::new());
        state.set_project_form_color(i32::try_from(palette::DEFAULT_INDEX).unwrap_or(0));
        state.set_project_form_editing(false);
        state.set_project_form_open(true);
    }

    fn edit_project(&self) {
        let Some(id) = self.nav().project else { return };
        let project = self.core.borrow().project(id);
        let project = match project {
            Ok(project) => project,
            Err(e) => return tracing::warn!("cannot edit project {}: {e}", id.0),
        };
        let index = palette::index_of(&project.color).unwrap_or(palette::DEFAULT_INDEX);
        let state = self.state();
        state.set_project_form_name(project.name.into());
        state.set_project_form_color(i32::try_from(index).unwrap_or(0));
        state.set_project_form_editing(true);
        state.set_project_form_open(true);
    }

    fn save_project(&self, name: SharedString, color: i32) {
        let Some(name) = clean(&name) else { return };
        let hex = palette::hex(usize::try_from(color).unwrap_or(palette::DEFAULT_INDEX));
        let editing = self.state().get_project_form_editing();
        if let (true, Some(id)) = (editing, self.nav().project) {
            if self.mutate("edit the project", |c| c.update_project(id, &name, hex)) {
                self.state().set_project_form_open(false);
            }
            return;
        }
        let created = self.core.borrow_mut().create_project(&name, hex);
        match created {
            Ok(id) => self.go_to_project(id),
            Err(e) => tracing::warn!("failed to create the project: {e}"),
        }
    }

    fn ask_delete_project(&self) {
        let Some(id) = self.nav().project else { return };
        let name = self.core.borrow().project(id).map(|p| p.name);
        let name = match name {
            Ok(name) => name,
            Err(e) => return tracing::warn!("cannot delete project {}: {e}", id.0),
        };
        self.ask(
            Pending::DeleteProject(id),
            &format!("Apagar o projeto “{name}”?"),
            "As tarefas continuam no Clocky, sem projeto: aparecem na busca e no seletor de \
             tarefas. As sessões ficam nos relatórios.",
            "Apagar projeto",
        );
    }

    pub(super) fn ask(&self, pending: Pending, title: &str, text: &str, action: &str) {
        self.pending.set(Some(pending));
        let state = self.state();
        state.set_confirm_archive(false);
        state.set_confirm_title(title.into());
        state.set_confirm_text(text.into());
        state.set_confirm_action(action.into());
        state.set_confirm_open(true);
    }

    fn close_confirm(&self) {
        self.pending.set(None);
        self.state().set_confirm_open(false);
    }

    fn confirm_accepted(&self) {
        let pending = self.pending.take();
        self.close_confirm();
        match pending {
            Some(Pending::DeleteProject(id)) => {
                self.leave(true);
                self.mutate("delete the project", |c| c.delete_project(id));
                self.show_all();
            }
            Some(Pending::DeleteTask(id)) => {
                self.leave(false);
                self.mutate("delete the task", |c| c.delete_task(id));
                self.back_to_project();
            }
            Some(Pending::ArchiveProject(id)) => self.archive_project(id),
            Some(Pending::ArchiveTask(id)) => self.archive_task(id),
            Some(Pending::DeleteSession(id)) => self.delete_session(id),
            Some(Pending::DeletePreset(id)) => self.delete_preset(id),
            None => {}
        }
    }
}

/// Tasks.
impl Manage {
    fn create_task(&self, name: SharedString) {
        let Some(name) = clean(&name) else { return };
        let Some(project) = self.nav().project else {
            return;
        };
        // An active search would hide the new task right away: clear it first.
        let had_search = !std::mem::take(&mut self.nav.borrow_mut().search).is_empty();
        if had_search {
            self.state().set_search(SharedString::new());
        }
        if self.mutate("create the task", |c| {
            c.create_task(Some(project), &name).map(drop)
        }) {
            // The typing row stays open for the next task.
            self.state().set_task_draft(SharedString::new());
        }
    }

    fn set_task_done(&self, id: i32, done: bool) {
        let Some(id) = db_id(id).map(TaskId) else {
            return;
        };
        self.mutate("complete or reopen the task", |c| c.set_task_done(id, done));
    }

    fn toggle_current_task(&self) {
        let Some(id) = self.nav().task else { return };
        let done = self.state().get_task_done();
        self.mutate("complete or reopen the task", |c| {
            c.set_task_done(id, !done)
        });
    }

    fn rename_task(&self, name: SharedString) {
        let (Some(id), Some(name)) = (self.nav().task, clean(&name)) else {
            return;
        };
        if self.mutate("rename the task", |c| c.rename_task(id, &name)) {
            self.state().set_renaming_task(false);
        }
    }

    fn move_task(&self, project: i32) {
        if let Some(project) = db_id(project) {
            self.move_task_to(Some(ProjectId(project)));
        }
    }

    /// Moves the task of the task page (`None` = "Sem projeto").
    fn move_task_to(&self, project: Option<ProjectId>) {
        let Some(id) = self.nav().task else { return };
        if self.mutate("move the task", |c| c.move_task(id, project)) {
            self.navigate(Nav {
                project,
                ..self.nav()
            });
        }
    }

    fn ask_delete_task(&self) {
        let Some(id) = self.nav().task else { return };
        let name = self.core.borrow().task(id).map(|t| t.name);
        let name = match name {
            Ok(name) => name,
            Err(e) => return tracing::warn!("cannot delete task {}: {e}", id.0),
        };
        self.ask(
            Pending::DeleteTask(id),
            &format!("Apagar a tarefa “{name}”?"),
            "As subtarefas também são apagadas. As sessões continuam nos relatórios como \
             “Sem tarefa”.",
            "Apagar tarefa",
        );
    }
}

/// Subtasks (checked outside a timer: no run).
impl Manage {
    fn add_subtask(&self, name: SharedString) {
        let (Some(task), Some(name)) = (self.nav().task, clean(&name)) else {
            return;
        };
        if self.mutate("add the subtask", |c| c.add_subtask(task, &name).map(drop)) {
            self.state().set_subtask_draft(SharedString::new());
        }
    }

    fn set_subtask_done(&self, id: i32, done: bool) {
        let Some(id) = db_id(id).map(SubtaskId) else {
            return;
        };
        self.mutate("check or uncheck the subtask", |c| {
            c.set_subtask_done(id, done)
        });
    }

    fn rename_subtask(&self, id: i32, name: SharedString) {
        let (Some(id), Some(name)) = (db_id(id).map(SubtaskId), clean(&name)) else {
            return;
        };
        self.mutate("rename the subtask", |c| c.rename_subtask(id, &name));
    }

    fn delete_subtask(&self, id: i32) {
        let Some(id) = db_id(id).map(SubtaskId) else {
            return;
        };
        self.mutate("delete the subtask", |c| c.delete_subtask(id));
    }
}
