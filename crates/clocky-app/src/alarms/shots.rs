//! Screenshots of the alarm screens (`CLOCKY_SCREENSHOT_DIR` + `CLOCKY_DEMO=1`), after the timer
//! panel captures: the editor (weekdays and once), Ativos with alarms, the ringing overlay (on
//! time and late), the missed notice and the task page with an alarm link. Rings are injected
//! into the queue (a freshly seeded alarm is never pending); nothing takes the focus.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use clocky_core::store::Alarm;
use slint::{ComponentHandle, Model};

use super::{Alarms, Ringing};
use crate::app::App;
use crate::manage_view::Nav;
use crate::panel::PanelKind;
use crate::timers::TimerPanels;
use crate::{screenshot, ui};

/// Time for a window to resize and render before a capture.
const SETTLE: Duration = Duration::from_millis(700);

#[derive(Debug, Clone, Copy)]
enum Window {
    Panel,
    Overlay,
    Manage,
}

#[derive(Debug, Clone, Copy)]
enum Scene {
    EditorWeekdays,
    EditorOnce,
    Overview,
    RingingOnTime,
    RingingLate,
    Missed,
    TaskPage,
    ProjectPage,
}

const SCENES: [(&str, Scene, Window); 9] = [
    ("alarme-editor", Scene::EditorWeekdays, Window::Panel),
    ("alarme-uma-vez", Scene::EditorOnce, Window::Panel),
    ("ativos-alarmes", Scene::Overview, Window::Panel),
    ("alarme-tocando", Scene::RingingOnTime, Window::Overlay),
    ("ativos-alarme-tocando", Scene::Overview, Window::Panel),
    ("alarme-atrasado", Scene::RingingLate, Window::Overlay),
    ("alarme-perdido", Scene::Missed, Window::Panel),
    ("projeto-alarme", Scene::ProjectPage, Window::Manage),
    ("tarefa-alarme", Scene::TaskPage, Window::Manage),
];

struct Run {
    app: Rc<App>,
    alarms: Rc<Alarms>,
    timers: Rc<TimerPanels>,
    dir: PathBuf,
    done: fn(),
    steps: Vec<(bool, usize)>,
}

/// Starts the capture chain; `done` runs after the last capture.
pub fn start(app: &Rc<App>, alarms: Rc<Alarms>, timers: Rc<TimerPanels>, dir: PathBuf, done: fn()) {
    let steps = [true, false]
        .into_iter()
        .flat_map(|dark| (0..SCENES.len()).map(move |i| (dark, i)))
        .collect();
    let run = Rc::new(Run {
        app: app.clone(),
        alarms,
        timers,
        dir,
        done,
        steps,
    });
    trim_timers(&run);
    next(run, 0);
}

fn next(run: Rc<Run>, index: usize) {
    slint::Timer::single_shot(SETTLE, move || step(&run, index));
}

fn step(run: &Rc<Run>, index: usize) {
    let Some(&(dark, scene)) = run.steps.get(index) else {
        clear(run);
        (run.done)();
        return;
    };
    let Some(&(name, scene, window)) = SCENES.get(scene) else {
        return;
    };
    run.app.set_dark(dark);
    show(run, scene);
    let run = run.clone();
    slint::Timer::single_shot(SETTLE, move || {
        let theme = if dark { "dark" } else { "light" };
        let target = match window {
            Window::Panel => run.app.panel.ui.window(),
            Window::Overlay => run.alarms.overlay_window(),
            Window::Manage => run.app.manage.ui.window(),
        };
        screenshot::save_all(&run.dir, theme, &[(name, target)]);
        // The last scene of a theme: no ring or notice carries over.
        if name == "tarefa-alarme" {
            clear(&run);
        }
        next(run, index + 1);
    });
}

fn show(run: &Run, scene: Scene) {
    let demo = demo_alarms(run);
    match scene {
        Scene::EditorWeekdays => {
            if let Some(standup) = demo.iter().find(|a| !a.repeat.is_once()) {
                run.app.open_alarm(standup.id);
            }
        }
        Scene::EditorOnce => new_once(run),
        Scene::Overview => run.app.open_panel(PanelKind::Overview, false),
        Scene::RingingOnTime => ring(run, &demo, &[None]),
        Scene::RingingLate => {
            clear(run);
            ring(run, &demo, &[Some(Duration::from_mins(12)), None]);
        }
        Scene::Missed => {
            clear(run);
            run.timers
                .add_notices(&["Alarme perdido às 06:10 · Remédio".to_owned()]);
            run.app.open_panel(PanelKind::Overview, false);
        }
        Scene::TaskPage => task_page(run, &demo, true),
        Scene::ProjectPage => task_page(run, &demo, false),
    }
}

/// "Novo alarme" filled like screen 11, through the editor's callbacks.
fn new_once(run: &Run) {
    run.app.open_panel(PanelKind::Alarm, false);
    let Some(window) = run.app.panel.ui.as_weak().upgrade() else {
        return;
    };
    let g = window.global::<ui::AlarmUi>();
    g.invoke_name_edited("Daily standup".into());
    g.invoke_message_edited("Hora da daily!".into());
    g.invoke_hour_edited("07".into());
    g.invoke_minute_edited("30".into());
    // Any stepper change pushes the whole draft back to the fields.
    g.invoke_bump_snooze(true);
    g.invoke_bump_snooze(false);
}

fn demo_alarms(run: &Run) -> Vec<Alarm> {
    run.app.core.borrow().alarms().unwrap_or_default()
}

/// Rings the demo alarms (the first one late by `lates[0]`, and so on).
fn ring(run: &Run, demo: &[Alarm], lates: &[Option<Duration>]) {
    let now = run.app.core.borrow().now();
    let rings = demo
        .iter()
        .zip(lates)
        .map(|(alarm, late)| Ringing {
            id: alarm.id,
            due: now,
            name: alarm.name.clone(),
            message: alarm.message.clone(),
            time: alarm.time,
            task: alarm.task,
            sound: alarm.sound.clone(),
            snooze_minutes: alarm.snooze_minutes,
            late: *late,
        })
        .collect();
    run.alarms.present(rings);
}

fn clear(run: &Run) {
    for alarm in demo_alarms(run) {
        run.alarms.dismiss(alarm.id);
    }
    run.timers.clear_notice();
}

/// The task page of the first demo alarm's task ("standup diário"), or its project page.
fn task_page(run: &Run, demo: &[Alarm], task_page: bool) {
    let Some(task) = demo.first().and_then(|a| a.task) else {
        return;
    };
    let project = run
        .app
        .core
        .borrow()
        .task(task)
        .ok()
        .and_then(|t| t.project);
    run.app.manage.navigate(Nav {
        project,
        task: task_page.then_some(task),
        ..Nav::default()
    });
}

/// Leaves one pomodoro of the timer captures running, so the alarm cards fit in the Ativos
/// capture (the list scrolls past 640 px): the ringing tea is dismissed and the other timers
/// stopped, through the Ativos buttons. The leftover notice is closed too.
fn trim_timers(run: &Run) {
    run.timers.clear_notice();
    run.app.open_panel(PanelKind::Overview, false);
    let Some(window) = run.app.panel.ui.as_weak().upgrade() else {
        return;
    };
    let g = window.global::<ui::TimerUi>();
    let cards: Vec<ui::ActiveCard> = g.get_cards().iter().collect();
    for card in cards.iter().filter(|c| c.kind != "alarm") {
        if card.ringing {
            g.invoke_card_dismiss(card.id);
        } else if card.name != "Pomodoro · Clássico" {
            g.invoke_card_stop(card.id);
        }
    }
    run.timers.clear_notice();
}
