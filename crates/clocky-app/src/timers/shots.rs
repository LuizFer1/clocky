//! Screenshots of the timer panels (`CLOCKY_SCREENSHOT_DIR` + `CLOCKY_DEMO=1`): a chain of
//! single-shot timers that sets a view, waits for the window to resize and render, saves only
//! the panel window and moves on. Nothing takes the focus and nothing is typed.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use super::TimerPanels;
use super::actions_overview::TaskTarget;
use super::demo::{self, DemoTimers};
use super::state::View;
use crate::panel::PanelKind;
use crate::{screenshot, ui};

/// Time for the window to take its new size and render before a capture.
const SETTLE: Duration = Duration::from_millis(600);

/// What a capture shows.
#[derive(Debug, Clone, Copy)]
enum Scene {
    StopwatchRunning,
    CountdownConfig,
    CountdownRunning,
    PomodoroList,
    WizardStep1,
    PomodoroTimer,
    Overview,
    /// The temporary task chooser over the stopwatch.
    Chooser,
    Forgotten,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Theme(bool),
    Seed,
    /// Lowers the forgotten limit so the demo stopwatch counts as forgotten.
    Forget,
    Shot(&'static str, Scene),
}

const SCENES: [(&str, Scene); 8] = [
    ("cronometro-running", Scene::StopwatchRunning),
    ("temporizador-config", Scene::CountdownConfig),
    ("temporizador-running", Scene::CountdownRunning),
    ("pomodoro-list", Scene::PomodoroList),
    ("pomodoro-wizard-step1", Scene::WizardStep1),
    ("pomodoro-timer", Scene::PomodoroTimer),
    ("ativos", Scene::Overview),
    ("vincular-tarefa", Scene::Chooser),
];

fn plan() -> Vec<Step> {
    let mut steps = vec![
        Step::Theme(true),
        Step::Shot("ativos-empty", Scene::Overview),
        Step::Theme(false),
        Step::Shot("ativos-empty", Scene::Overview),
        Step::Seed,
    ];
    for dark in [true, false] {
        steps.push(Step::Theme(dark));
        steps.extend(SCENES.iter().map(|(name, scene)| Step::Shot(name, *scene)));
    }
    // Last, because the dialog stays up once the stopwatch counts as forgotten.
    steps.extend([
        Step::Forget,
        Step::Shot("esquecido-dialog", Scene::Forgotten),
        Step::Theme(true),
        Step::Shot("esquecido-dialog", Scene::Forgotten),
    ]);
    steps
}

/// What the capture chain needs from the app.
pub struct ShotContext {
    pub panels: Rc<TimerPanels>,
    pub window: slint::Weak<ui::PanelWindow>,
    pub dir: PathBuf,
    /// Switches every window's theme.
    pub set_dark: Box<dyn Fn(bool)>,
    /// Opens the panel window (without focus).
    pub open_panel: Box<dyn Fn(PanelKind)>,
    /// Called at the end (quits the app).
    pub done: Box<dyn Fn()>,
}

struct Run {
    ctx: ShotContext,
    steps: Vec<Step>,
    demo: std::cell::Cell<Option<DemoTimers>>,
    dark: std::cell::Cell<bool>,
}

/// Starts the capture chain.
pub fn start(ctx: ShotContext) {
    (ctx.open_panel)(PanelKind::Overview);
    let run = Rc::new(Run {
        ctx,
        steps: plan(),
        demo: std::cell::Cell::new(None),
        dark: std::cell::Cell::new(true),
    });
    next(run, 0);
}

fn next(run: Rc<Run>, index: usize) {
    slint::Timer::single_shot(SETTLE, move || step(&run, index));
}

fn step(run: &Rc<Run>, index: usize) {
    let Some(step) = run.steps.get(index).copied() else {
        (run.ctx.done)();
        return;
    };
    match step {
        Step::Theme(dark) => {
            run.dark.set(dark);
            (run.ctx.set_dark)(dark);
        }
        Step::Seed => seed(run),
        Step::Forget => forget(run),
        Step::Shot(name, scene) => {
            show(run, scene);
            let run = run.clone();
            slint::Timer::single_shot(SETTLE, move || {
                capture(&run, name);
                next(run, index + 1);
            });
            return;
        }
    }
    next(run.clone(), index + 1);
}

fn seed(run: &Run) {
    let seeded = demo::seed(&mut run.ctx.panels.handle().borrow_mut());
    match seeded {
        Ok(timers) => run.demo.set(Some(timers)),
        Err(e) => tracing::error!("demo seeding failed: {e:#}"),
    }
    // Rings the due tea countdown and stops the deep-work pomodoro at its break (silently).
    let outcome = run.ctx.panels.tick();
    tracing::info!("demo seeded: {} active timers", outcome.active_count);
}

fn forget(run: &Run) {
    let result = run
        .ctx
        .panels
        .handle()
        .borrow_mut()
        .tracker_mut()
        .store_mut()
        .set_forgotten_limit(Duration::from_hours(1));
    if let Err(e) = result {
        tracing::error!("cannot lower the forgotten limit: {e}");
    }
    let _outcome = run.ctx.panels.tick();
}

fn show(run: &Run, scene: Scene) {
    let panels = &run.ctx.panels;
    let demo = run.demo.get();
    if let Some(window) = run.ctx.window.upgrade() {
        window.global::<ui::TimerUi>().invoke_chooser_cancel();
    }
    match scene {
        Scene::StopwatchRunning => panels.show(PanelKind::Stopwatch),
        Scene::CountdownConfig => panels.set_view(View::CountdownConfig),
        Scene::CountdownRunning => match demo {
            Some(d) => panels.open_card(d.countdown),
            None => panels.show(PanelKind::Countdown),
        },
        Scene::PomodoroList => panels.set_view(View::PomodoroList),
        Scene::WizardStep1 => panels.wizard_open(),
        Scene::PomodoroTimer => match demo {
            Some(d) => panels.open_card(d.focus),
            None => panels.show(PanelKind::Pomodoro),
        },
        Scene::Overview => panels.show(PanelKind::Overview),
        Scene::Chooser => {
            panels.show(PanelKind::Stopwatch);
            panels.choose_task(TaskTarget::Stopwatch);
        }
        Scene::Forgotten => panels.render(),
    }
}

fn capture(run: &Run, name: &str) {
    let Some(window) = run.ctx.window.upgrade() else {
        return;
    };
    let theme = if run.dark.get() { "dark" } else { "light" };
    screenshot::save_all(&run.ctx.dir, theme, &[(name, window.window())]);
}
