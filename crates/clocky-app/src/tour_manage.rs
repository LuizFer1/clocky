//! M7b screenshot tour, after the timer panel captures of a demo run: Relatórios (each view,
//! an empty one and the Sessão detail), Configurações, the dialogs, the narrow layout, and
//! every window after the theme is switched from Configurações. Dark pass, then light; then
//! the app quits. Nothing here takes the focus or simulates input.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use chrono::{Days, NaiveDate};
use clocky_core::domain::{ProjectId, TaskId};
use clocky_core::store::DEFAULT_FORGOTTEN_LIMIT;
use slint::{ComponentHandle, LogicalSize};

use crate::app::{App, quit};
use crate::manage_view::Nav;
use crate::panel::PanelKind;
use crate::reports_view::{Back, Range, ReportNav, Tab};
use crate::{screenshot, ui};

const STEP_DELAY: Duration = Duration::from_millis(900);
const WIDE: (f32, f32) = (1120.0, 720.0);
/// The narrow layout (screen 74), tall enough for the whole page.
const NARROW: (f32, f32) = (480.0, 1600.0);

#[derive(Debug, Clone, Copy)]
enum Shot {
    Manage,
    /// Dock, panel and manage window (after the theme switch).
    All,
    /// Nothing to save (a step that only restores state).
    None,
}

struct Step {
    name: &'static str,
    shot: Shot,
    /// Only in the dark pass (the theme switch runs once).
    dark_only: bool,
    prepare: fn(&App, &Targets),
}

/// What the steps show: today, the demo's Friday (its session with pauses and milestones),
/// the first project and its first task.
#[derive(Debug, Clone, Copy)]
struct Targets {
    today: NaiveDate,
    friday: NaiveDate,
    session: Option<i32>,
    project: Option<ProjectId>,
    task: Option<TaskId>,
}

const fn step(name: &'static str, prepare: fn(&App, &Targets)) -> Step {
    Step {
        name,
        shot: Shot::Manage,
        dark_only: false,
        prepare,
    }
}

const STEPS: &[Step] = &[
    step("reports-weekly", |app, t| {
        resize(app, WIDE);
        app.manage.show_reports(ReportNav::new(t.today));
    }),
    step("reports-overview", |app, t| {
        app.manage
            .show_reports(reports(Tab::Overview, Range::Week, t.today));
    }),
    step("reports-overview-month", |app, t| {
        app.manage
            .show_reports(reports(Tab::Overview, Range::Month, t.today));
    }),
    step("reports-daily", |app, t| {
        app.manage
            .show_reports(reports(Tab::Daily, Range::Week, t.friday));
    }),
    step("reports-daily-today", |app, t| {
        app.manage
            .show_reports(reports(Tab::Daily, Range::Week, t.today));
    }),
    step("reports-empty", |app, t| {
        let old = t.today.checked_sub_days(Days::new(35)).unwrap_or(t.today);
        app.manage.show_reports(ReportNav::new(old));
    }),
    step("reports-session", |app, t| {
        app.manage
            .show_reports(reports(Tab::Daily, Range::Week, t.friday));
        if let Some(id) = t.session {
            app.manage.open_session(id, Back::Reports);
        }
    }),
    step("dialog-delete-session", |app, _| {
        app.manage.ask_delete_session();
    }),
    step("settings", |app, _| {
        cancel_dialog(app);
        // The timer captures lowered it to 1 h for the forgotten-stopwatch dialog.
        let reset = app
            .core
            .borrow_mut()
            .set_forgotten_limit(DEFAULT_FORGOTTEN_LIMIT);
        if let Err(e) = reset {
            tracing::warn!("cannot reset the forgotten limit: {e}");
        }
        app.manage.open_settings();
    }),
    step("settings-full", |app, _| resize(app, (WIDE.0, 1180.0))),
    step("dialog-archive-project", |app, t| {
        resize(app, WIDE);
        app.manage.navigate(Nav {
            project: t.project,
            ..Nav::default()
        });
        app.manage.ask_archive_project();
    }),
    step("dialog-archive-task", |app, t| {
        cancel_dialog(app);
        app.manage.navigate(Nav {
            project: t.project,
            task: t.task,
            ..Nav::default()
        });
        app.manage.ask_archive_task();
    }),
    step("dialog-delete-task", |app, _| {
        cancel_dialog(app);
        state(app).invoke_delete_task();
    }),
    step("reports-weekly-narrow", |app, t| {
        cancel_dialog(app);
        resize(app, NARROW);
        app.manage.show_reports(ReportNav::new(t.today));
    }),
    step("reports-daily-narrow", |app, t| {
        app.manage
            .show_reports(reports(Tab::Daily, Range::Week, t.friday));
    }),
    step("reports-session-narrow", |app, t| {
        if let Some(id) = t.session {
            app.manage.open_session(id, Back::Reports);
        }
    }),
    step("settings-narrow", |app, _| app.manage.open_settings()),
    Step {
        name: "after-theme-switch",
        shot: Shot::All,
        dark_only: true,
        prepare: |app, _| {
            resize(app, WIDE);
            app.manage.open_settings();
            app.manage
                .ui
                .global::<ui::SettingsState>()
                .invoke_set_theme(false);
            app.open_panel(PanelKind::Overview, false);
            app.dock.ui.set_open(true);
        },
    },
    Step {
        name: "theme-restore",
        shot: Shot::None,
        dark_only: true,
        prepare: |app, _| {
            app.manage
                .ui
                .global::<ui::SettingsState>()
                .invoke_set_theme(true);
        },
    },
];

fn reports(tab: Tab, range: Range, date: NaiveDate) -> ReportNav {
    ReportNav {
        tab,
        range,
        ..ReportNav::new(date)
    }
}

fn state(app: &App) -> ui::ManageState<'_> {
    app.manage.ui.global::<ui::ManageState>()
}

fn cancel_dialog(app: &App) {
    state(app).invoke_confirm_cancelled();
}

fn resize(app: &App, (width, height): (f32, f32)) {
    app.manage
        .ui
        .window()
        .set_size(LogicalSize::new(width, height));
}

struct Tour {
    dir: PathBuf,
    dark: Cell<bool>,
    targets: Targets,
}

/// Starts the tour on `app` (if it is still alive) and quits after the light pass.
pub fn run(app: &Weak<App>, dir: PathBuf) {
    let Some(app) = app.upgrade() else { return };
    if !app.manage.show() {
        tracing::warn!("the manage window could not be shown");
    }
    app.set_dark(true);
    let tour = Rc::new(Tour {
        dir,
        dark: Cell::new(true),
        targets: targets(&app),
    });
    next(&app, tour, 0);
}

fn targets(app: &App) -> Targets {
    let today = app.manage.today();
    let friday = today.checked_sub_days(Days::new(2)).unwrap_or(today);
    let core = app.core.borrow();
    let session = core
        .daily(friday, &chrono::Local)
        .ok()
        .and_then(|d| d.blocks.first().map(|b| b.session.0))
        .and_then(|id| i32::try_from(id).ok());
    let project = core
        .projects()
        .ok()
        .and_then(|p| p.first().map(|p| p.project.id));
    let task = project
        .and_then(|p| core.task_summaries(Some(p), false).ok())
        .and_then(|tasks| tasks.first().map(|s| s.task.id));
    Targets {
        today,
        friday,
        session,
        project,
        task,
    }
}

fn next(app: &Rc<App>, tour: Rc<Tour>, index: usize) {
    let Some(current) = STEPS.get(index) else {
        if tour.dark.replace(false) {
            app.set_dark(false);
            return next(app, tour, 0);
        }
        return quit();
    };
    if current.dark_only && !tour.dark.get() {
        return next(app, tour, index + 1);
    }
    (current.prepare)(app, &tour.targets);
    let weak = Rc::downgrade(app);
    slint::Timer::single_shot(STEP_DELAY, move || {
        let Some(app) = weak.upgrade() else { return };
        let theme = if tour.dark.get() { "dark" } else { "light" };
        match current.shot {
            Shot::Manage => {
                let windows = [(current.name, app.manage.ui.window())];
                screenshot::save_all(&tour.dir, theme, &windows);
            }
            Shot::All => {
                let dock = format!("{}-dock", current.name);
                let panel = format!("{}-panel", current.name);
                let manage = format!("{}-manage", current.name);
                let windows = [
                    (dock.as_str(), app.dock.ui.window()),
                    (panel.as_str(), app.panel.ui.window()),
                    (manage.as_str(), app.manage.ui.window()),
                ];
                screenshot::save_all(&tour.dir, "from-dark", &windows);
            }
            Shot::None => {}
        }
        next(&app, tour, index + 1);
    });
}
