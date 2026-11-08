//! Manage window (projects, tasks and subtasks). Closing it only hides it: the app stays in the
//! tray. Every change goes through [`AppCore`]; the Slint models are rebuilt from the core right
//! after it (no polling).

mod actions;
// M7b: Relatórios, Configurações and the archive confirmations.
mod archive;
mod reports;
mod settings;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use clocky_core::domain::{PomodoroPresetId, ProjectId, SessionId, TaskId};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::core::{AppCore, CoreResult};
use crate::manage_view::{self, ManageView, Nav, Page};
use crate::reports_view::ReportNav;
use crate::settings_view::Applied;
use crate::settings_view::presets::PresetDraft;
use crate::{autostart, chips, palette, ui};

/// Width used to wrap the "Mover para" chips (fits the 480px layout).
const MOVE_CHIPS_WIDTH: f32 = 380.0;

/// Timer a task page button asks for (M6b opens the panel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerStart {
    Pomodoro,
    Stopwatch,
}

/// A change waiting for the confirmation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    DeleteProject(ProjectId),
    DeleteTask(TaskId),
    ArchiveProject(ProjectId),
    ArchiveTask(TaskId),
    DeleteSession(SessionId),
    DeletePreset(PomodoroPresetId),
}

/// The pages of the sidebar footer, over the project pages of [`Nav`] (M7b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Projects,
    Reports,
    Settings,
}

type StartHook = Box<dyn Fn(TimerStart, TaskId)>;
type SettingsHook = Box<dyn Fn(Applied)>;

pub struct Manage {
    pub ui: ui::ManageWindow,
    core: Rc<RefCell<AppCore>>,
    nav: RefCell<Nav>,
    pending: Cell<Option<Pending>>,
    on_start_timer: RefCell<Option<StartHook>>,
    weak: Weak<Manage>,
    section: Cell<Section>,
    /// Relatórios, once opened.
    reports: RefCell<Option<ReportNav>>,
    autostart: RefCell<Box<dyn autostart::Autostart>>,
    on_settings: RefCell<Option<SettingsHook>>,
    /// The saved pomodoro open in the Configurações editor.
    preset_draft: RefCell<Option<PresetDraft>>,
}

impl Manage {
    pub fn new(core: Rc<RefCell<AppCore>>) -> anyhow::Result<Rc<Self>> {
        let ui = ui::ManageWindow::new()?;
        ui.window()
            .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
        let manage = Rc::new_cyclic(|weak| Self {
            ui,
            core,
            nav: RefCell::new(Nav::default()),
            pending: Cell::new(None),
            on_start_timer: RefCell::new(None),
            weak: weak.clone(),
            section: Cell::new(Section::Projects),
            reports: RefCell::new(None),
            autostart: RefCell::new(Box::new(autostart::Fake::default())),
            on_settings: RefCell::new(None),
            preset_draft: RefCell::new(None),
        });
        let colors: Vec<slint::Color> = palette::COLORS
            .iter()
            .map(|(_, hex)| palette::color(hex))
            .collect();
        manage
            .state()
            .set_palette(ModelRc::new(VecModel::from(colors)));
        manage.connect();
        manage.refresh();
        Ok(manage)
    }

    fn state(&self) -> ui::ManageState<'_> {
        self.ui.global::<ui::ManageState>()
    }

    /// What "Iniciar pomodoro" / "Iniciar cronômetro" do. Until M6b wires the timer panels,
    /// the app only logs the request.
    pub fn set_on_start_timer(&self, hook: impl Fn(TimerStart, TaskId) + 'static) {
        *self.on_start_timer.borrow_mut() = Some(Box::new(hook));
    }

    /// Shows the window without taking the focus, with fresh data (other windows, such as the
    /// task picker, may have changed it). Returns `false` when it cannot be shown.
    pub fn show(&self) -> bool {
        self.refresh();
        if let Err(e) = self.ui.show() {
            tracing::warn!("failed to show the manage window: {e}");
            return false;
        }
        true
    }

    /// Shows the window and brings it to the front. Only for user actions (tray, second launch,
    /// radial menu): on Windows `focus_window` simulates an Alt key press to get past the
    /// foreground lock.
    pub fn bring_to_front(&self) {
        if !self.show() {
            return;
        }
        self.ui.window().with_winit_window(|w| {
            w.set_minimized(false);
            w.focus_window();
        });
    }

    pub fn set_theme(&self, mode: crate::settings::ThemeMode) {
        mode.apply(&self.ui.global::<ui::Theme>());
    }

    /// Moves to `nav` (screenshot hook and navigation callbacks) and rebuilds the view.
    pub fn navigate(&self, nav: Nav) {
        self.section.set(Section::Projects);
        *self.nav.borrow_mut() = nav;
        let state = self.state();
        state.set_adding_task(false);
        state.set_renaming_task(false);
        state.set_moving_task(false);
        state.set_project_form_open(false);
        let search = SharedString::from(self.nav.borrow().search.as_str());
        state.set_search(search);
        self.refresh();
    }

    /// The current navigation.
    pub fn nav(&self) -> Nav {
        self.nav.borrow().clone()
    }

    /// Rebuilds every model from the core. The core borrow ends before Slint is touched.
    pub fn refresh(&self) {
        let nav = self.nav();
        let built = {
            let core = self.core.borrow();
            manage_view::build(&core, &nav, &chrono::Local)
        };
        let view = match built {
            Ok(view) => view,
            Err(e) if nav.task.is_some() => {
                // The task is gone (deleted or archived): back to its project.
                tracing::warn!("task page unavailable, back to the project: {e}");
                self.nav.borrow_mut().task = None;
                return self.refresh();
            }
            Err(e) => {
                tracing::error!("failed to read the manage data: {e}");
                return;
            }
        };
        if view.page == Page::AllProjects && nav.project.is_some() {
            // The project was archived or deleted.
            self.nav.borrow_mut().project = None;
        }
        self.apply(&view);
    }

    fn apply(&self, view: &ManageView) {
        let state = self.state();
        state.set_page(view.page as i32);
        state.set_projects(model(view.projects.clone()));
        state.set_crumb(view.crumb.as_str().into());
        state.set_heading(view.heading.as_str().into());
        state.set_subheading(view.subheading.as_str().into());
        state.set_has_heading_color(view.heading_color.is_some());
        state.set_heading_color(view.heading_color.unwrap_or_default());
        state.set_tasks(model(view.tasks.clone()));
        let show_completed = self.nav.borrow().show_completed;
        state.set_show_completed(show_completed);
        if let Some(task) = &view.task {
            self.apply_task(task, view);
        }
        let section = self.section.get();
        if section != Section::Projects {
            // No project row stays selected over Relatórios and Configurações.
            let items: Vec<ui::ProjectItem> = view
                .projects
                .iter()
                .map(|p| ui::ProjectItem {
                    selected: false,
                    ..p.clone()
                })
                .collect();
            state.set_projects(model(items));
        }
        match section {
            Section::Projects => {}
            Section::Reports => self.apply_reports(),
            Section::Settings => self.apply_settings(),
        }
    }

    fn apply_task(&self, task: &manage_view::TaskDetail, view: &ManageView) {
        let state = self.state();
        state.set_task_done(task.done);
        state.set_task_project(SharedString::from(task.project.as_str()));
        state.set_task_total(task.total.as_str().into());
        state.set_subtasks(model(task.subtasks.clone()));
        state.set_checklist(task.checklist.as_str().into());
        state.set_checklist_progress(task.progress);
        state.set_links(model(task.links.clone()));
        state.set_sessions(model(task.sessions.clone()));
        // The project items already mark the task's project as selected.
        let current = self.nav.borrow().project;
        let mut items = view.projects.clone();
        items.push(chips::no_project_chip(current.is_none()));
        state.set_move_chips(chips::rows(&items, MOVE_CHIPS_WIDTH));
    }

    /// Runs one change on the core (short mutable borrow), logs a failure and refreshes.
    fn mutate(&self, what: &str, change: impl FnOnce(&mut AppCore) -> CoreResult<()>) -> bool {
        let result = change(&mut self.core.borrow_mut());
        let ok = match result {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("failed to {what}: {e}");
                false
            }
        };
        self.refresh();
        ok
    }

    /// Wraps a method as a Slint callback holding only a weak reference.
    fn handler0(&self, f: fn(&Self)) -> impl FnMut() + 'static {
        let weak = self.weak.clone();
        move || {
            if let Some(manage) = weak.upgrade() {
                f(&manage);
            }
        }
    }

    fn handler1<A: 'static>(&self, f: fn(&Self, A)) -> impl FnMut(A) + 'static {
        let weak = self.weak.clone();
        move |a| {
            if let Some(manage) = weak.upgrade() {
                f(&manage, a);
            }
        }
    }

    fn handler2<A: 'static, B: 'static>(&self, f: fn(&Self, A, B)) -> impl FnMut(A, B) + 'static {
        let weak = self.weak.clone();
        move |a, b| {
            if let Some(manage) = weak.upgrade() {
                f(&manage, a, b);
            }
        }
    }
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

/// A Slint id back to a database id (`None` for a negative value, which no row carries).
fn db_id(id: i32) -> Option<i64> {
    (id >= 0).then_some(i64::from(id))
}

/// A trimmed, non-empty name.
fn clean(name: &SharedString) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_ids_mean_none() {
        assert_eq!(db_id(-1), None);
        assert_eq!(db_id(7), Some(7));
    }

    #[test]
    fn names_are_trimmed_and_required() {
        assert_eq!(clean(&"  a b ".into()), Some("a b".to_owned()));
        assert_eq!(clean(&"   ".into()), None);
    }
}
