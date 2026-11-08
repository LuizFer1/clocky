//! Screenshot tour (`CLOCKY_SCREENSHOT_DIR`): walks through the windows, saving one PNG per
//! step in the dark theme and then in the light one, and quits. With `CLOCKY_DEMO=1` the
//! database holds the handoff's demo data, so the pages match screens 60-61 and 70-74.
//! Nothing here takes the focus or simulates input.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use clocky_core::domain::{ProjectId, TaskId};
use slint::{ComponentHandle, LogicalSize};

use crate::app::{App, quit};
use crate::manage_view::Nav;
use crate::panel::PanelKind;
use crate::{screenshot, ui};

/// Time for a step to be laid out and rendered before its snapshot.
const STEP_DELAY: Duration = Duration::from_millis(900);
/// Size of the handoff's manage screens (1120×720 logical).
const WIDE: (f32, f32) = (1120.0, 720.0);
/// Screen 74: the narrow layout, tall enough for the whole task page.
const NARROW: (f32, f32) = (480.0, 1340.0);
/// Search text of screen 61.
const CREATE_QUERY: &str = "relatório mensal";

/// Which window a step captures.
#[derive(Debug, Clone, Copy)]
enum Shot {
    Dock,
    Panel,
    Manage,
}

struct Step {
    name: &'static str,
    shot: Shot,
    prepare: fn(&App, &Targets),
}

/// The demo project and task the pages show (the first project and its first task).
#[derive(Debug, Clone, Copy, Default)]
struct Targets {
    project: Option<ProjectId>,
    task: Option<TaskId>,
}

const STEPS: &[Step] = &[
    Step {
        name: "dock",
        shot: Shot::Dock,
        prepare: |app, _| app.dock.ui.set_open(true),
    },
    Step {
        name: "panel",
        shot: Shot::Panel,
        prepare: |app, _| {
            app.panel
                .open(PanelKind::Overview, app.dock.anchor(), false);
        },
    },
    Step {
        name: "manage-all-projects",
        shot: Shot::Manage,
        prepare: |app, _| {
            resize(app, WIDE);
            app.manage.navigate(Nav::default());
        },
    },
    Step {
        name: "manage-new-project",
        shot: Shot::Manage,
        prepare: |app, _| {
            app.manage
                .ui
                .global::<ui::ManageState>()
                .invoke_new_project();
        },
    },
    Step {
        name: "manage-project",
        shot: Shot::Manage,
        prepare: |app, t| {
            app.manage.navigate(Nav {
                project: t.project,
                ..Nav::default()
            });
        },
    },
    Step {
        name: "manage-task",
        shot: Shot::Manage,
        prepare: |app, t| {
            app.manage.navigate(Nav {
                project: t.project,
                task: t.task,
                ..Nav::default()
            });
        },
    },
    Step {
        name: "manage-narrow",
        shot: Shot::Manage,
        prepare: |app, _| resize(app, NARROW),
    },
    Step {
        name: "picker",
        shot: Shot::Panel,
        prepare: |app, t| {
            resize(app, WIDE);
            app.panel
                .open(PanelKind::Stopwatch, app.dock.anchor(), false);
            app.panel.pick_task(t.task, |choice| {
                tracing::info!("screenshot picker returned {:?}", choice.map(|t| t.0));
            });
        },
    },
    Step {
        name: "picker-search-create",
        shot: Shot::Panel,
        prepare: |app, _| app.panel.picker.set_query(CREATE_QUERY),
    },
];

struct Tour {
    dir: PathBuf,
    dark: Cell<bool>,
    targets: Targets,
}

/// Starts the tour once the windows had time to appear.
pub fn run(app: &Rc<App>, dir: PathBuf) {
    if !app.manage.show() {
        tracing::warn!("the manage window could not be shown");
    }
    let tour = Rc::new(Tour {
        dir,
        dark: Cell::new(true),
        targets: targets(app),
    });
    let weak = Rc::downgrade(app);
    slint::Timer::single_shot(STEP_DELAY * 2, move || {
        if let Some(app) = weak.upgrade() {
            step(&app, tour, 0);
        }
    });
}

fn targets(app: &App) -> Targets {
    let core = app.core.borrow();
    let project = core
        .projects()
        .ok()
        .and_then(|p| p.first().map(|p| p.project.id));
    let task = project
        .and_then(|p| core.task_summaries(Some(p), false).ok())
        .and_then(|tasks| tasks.first().map(|s| s.task.id));
    Targets { project, task }
}

fn resize(app: &App, (width, height): (f32, f32)) {
    app.manage
        .ui
        .window()
        .set_size(LogicalSize::new(width, height));
}

/// Prepares step `index`, waits, saves it and goes on; after the last step, switches to the
/// light theme (or quits after the light pass).
fn step(app: &Rc<App>, tour: Rc<Tour>, index: usize) {
    let Some(current) = STEPS.get(index) else {
        if tour.dark.replace(false) {
            app.set_dark(false);
            return step(app, tour, 0);
        }
        return quit();
    };
    (current.prepare)(app, &tour.targets);
    let weak = Rc::downgrade(app);
    slint::Timer::single_shot(STEP_DELAY, move || {
        let Some(app) = weak.upgrade() else { return };
        let window = match current.shot {
            Shot::Dock => app.dock.ui.window(),
            Shot::Panel => app.panel.ui.window(),
            Shot::Manage => app.manage.ui.window(),
        };
        let theme = if tour.dark.get() { "dark" } else { "light" };
        screenshot::save_all(&tour.dir, theme, &[(current.name, window)]);
        step(&app, tour, index + 1);
    });
}
