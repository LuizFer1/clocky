//! The timer panels (M6b): Cronômetro, Temporizador, Pomodoro and Ativos, driven by the core
//! `Tracker`.
//!
//! [`TimerPanels`] shares the [`AppCore`] (which owns the tracker), owns the panel state, fills
//! the `TimerUi` global and answers its callbacks. Everything runs on the UI thread. Borrows of
//! the core and the state
//! are short and never held while Slint is called: a frame is built first, then applied.

mod actions;
mod actions_overview;
mod actions_pomodoro;
mod alerts;
mod apply;
mod cards;
mod chooser;
mod connect;
mod convert;
pub mod demo;
mod frame;
mod frame_pomodoro;
mod hms;
mod names;
// M7a: the Alarme panel and the alarm cards of Ativos.
mod alarm_view;
mod overview_alarms;
mod pomodoro_view;
mod segments;
pub mod shots;
mod start;
mod state;
mod wizard;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use clocky_core::domain::{TaskId, TimerId};
use clocky_core::engine::timers::{ActiveTimer, Timer, TimersEvent};
use clocky_core::store::StoreError;
use clocky_core::time::Timestamp;
use clocky_core::tracker::{TrackerError, TrackerEvent};
use slint::ComponentHandle;

pub use alerts::{Alerts, SilentAlerts, SystemAlerts};
use alerts::{TimerInfo, alert_for};
use apply::Models;
use cards::{Kind, Ringing};
pub use chooser::{PanelChooser, TaskChooser};
use names::Names;
use state::{PomodoroPick, State, View};

use crate::core::{AppCore, Reopened};
use crate::format;
use crate::panel::PanelKind;
use crate::ui;

/// What the app does after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickOutcome {
    /// The dock badge: the number of active timers.
    pub active_count: i32,
    /// A panel to open on its own (the forgotten-stopwatch dialog).
    pub open: Option<PanelKind>,
}

pub struct TimerPanels {
    ui: slint::Weak<ui::PanelWindow>,
    core: Rc<RefCell<AppCore>>,
    state: RefCell<State>,
    models: Models,
    alerts: Box<dyn Alerts>,
    chooser: Box<dyn TaskChooser>,
    /// `false` in screenshot runs: the tick never opens a panel by itself.
    interactive: bool,
    /// "Abrir menu" of the empty Ativos list (set by the app).
    on_open_menu: RefCell<Option<Rc<dyn Fn()>>>,
    /// M7a: the alarms (editor, Ativos cards, ringing).
    alarms: Rc<crate::alarms::Alarms>,
    weak: Weak<Self>,
}

/// How the panels reach the rest of the app.
pub struct Hooks {
    /// Opens the task picker for a timer.
    pub chooser: Box<dyn TaskChooser>,
    /// Sound and notification when a timer ends.
    pub alerts: Box<dyn Alerts>,
    /// `false` in screenshot runs: the tick never opens a panel by itself.
    pub interactive: bool,
    /// M7a: the alarms (editor, Ativos cards, ringing).
    pub alarms: Rc<crate::alarms::Alarms>,
}

impl TimerPanels {
    pub fn new(
        window: &ui::PanelWindow,
        core: Rc<RefCell<AppCore>>,
        reopened: &Reopened,
        hooks: Hooks,
    ) -> Rc<Self> {
        let Hooks {
            chooser,
            alerts,
            interactive,
            alarms,
        } = hooks;
        let models = Models::install(&window.global::<ui::TimerUi>());
        let panels = Rc::new_cyclic(|weak| Self {
            ui: window.as_weak(),
            core,
            state: RefCell::new(State::new()),
            models,
            alerts,
            chooser,
            interactive,
            on_open_menu: RefCell::new(None),
            alarms,
            weak: weak.clone(),
        });
        panels.load_wizard_defaults();
        panels.reopened(&reopened.events, &reopened.before);
        connect::connect(&panels);
        alarm_view::connect(&panels);
        panels
    }

    pub fn set_on_open_menu(&self, f: impl Fn() + 'static) {
        *self.on_open_menu.borrow_mut() = Some(Rc::new(f));
    }

    fn open_menu(&self) {
        let f = self.on_open_menu.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }

    /// The tracker, for the demo seeding of the screenshot hook.
    pub fn core(&self) -> &Rc<RefCell<AppCore>> {
        &self.core
    }

    /// Closes the task picker if a panel opened it (its choice is dropped).
    pub fn cancel_chooser(&self) {
        self.chooser.cancel();
    }

    /// A radial option was chosen: shows the right view of its panel.
    pub fn show(&self, kind: PanelKind) {
        // A picker left open for another timer must not apply its choice here.
        self.chooser.cancel();
        // M7a: "Alarme" opens a new alarm.
        if kind == PanelKind::Alarm {
            self.alarms.start_new();
        }
        {
            let handle = self.core.borrow();
            let timers = handle.tracker().timers();
            let mut state = self.state.borrow_mut();
            if std::mem::take(&mut state.notice_seen) {
                state.notice.clear();
            }
            state::forget_ended(&mut state, timers);
            state.view = match kind {
                PanelKind::Alarm => View::Alarm {
                    from_overview: false,
                },
                PanelKind::Stopwatch => View::Stopwatch,
                PanelKind::Countdown => View::CountdownRun,
                PanelKind::Pomodoro => {
                    state.pomodoro = PomodoroPick::Active(match state.pomodoro {
                        PomodoroPick::Active(selected) => selected,
                        PomodoroPick::Draft(_) => None,
                    });
                    View::PomodoroTimer
                }
                PanelKind::Overview => View::Overview,
            };
            state::normalize(&mut state, timers);
        }
        self.render();
    }

    /// The header's back button. `true` = handled inside the panel; `false` = back to the menu.
    pub fn back(&self) -> bool {
        self.chooser.cancel();
        let parent = self.state.borrow().view.parent();
        let Some(parent) = parent else {
            return false;
        };
        self.set_view(parent);
        true
    }

    fn set_view(&self, view: View) {
        self.state.borrow_mut().view = view;
        self.render();
    }

    /// 1 Hz: advances the tracker, rings what finished, checks for a forgotten stopwatch and
    /// refreshes the visible panel.
    pub fn tick(&self) -> TickOutcome {
        let result = {
            let mut handle = self.core.borrow_mut();
            let now = handle.now();
            let infos = infos_of_due(&handle, now);
            handle.tracker_mut().tick(now).map(|events| (events, infos))
        };
        match result {
            Ok((events, infos)) => {
                self.state.borrow_mut().tick_failing = false;
                self.ring(&events, &infos);
            }
            Err(e) => self.tick_failed(&e),
        }
        let open = self.check_forgotten();
        if self.is_visible() {
            self.render();
        }
        TickOutcome {
            active_count: self.active_count(),
            open,
        }
    }

    fn tick_failed(&self, error: &TrackerError) {
        // Memory is unchanged, so the next tick retries; log once per failure streak.
        if !std::mem::replace(&mut self.state.borrow_mut().tick_failing, true) {
            tracing::error!("tick failed (retrying every second): {error}");
        }
    }

    pub fn active_count(&self) -> i32 {
        let count = self.core.borrow().tracker().timers().iter().count();
        i32::try_from(count).unwrap_or(i32::MAX)
    }

    fn is_visible(&self) -> bool {
        self.ui.upgrade().is_some_and(|ui| ui.window().is_visible())
    }

    /// Plays the sound and notifies for countdowns and pomodoro phases that ended live.
    fn ring(&self, events: &[TrackerEvent], infos: &HashMap<TimerId, TimerInfo>) {
        for event in events {
            let TrackerEvent::Timer(event) = event else {
                continue;
            };
            let timer = match event {
                TimersEvent::CountdownFinished { timer, at } => {
                    self.add_ringing(*timer, *at, infos, false);
                    *timer
                }
                TimersEvent::PomodoroPhaseEnded { timer, .. } => *timer,
                TimersEvent::SessionCompleted { .. } => continue,
            };
            let info = self.info(timer, infos);
            if let Some(alert) = alert_for(event, &info) {
                self.alerts.alert(&alert);
            }
        }
    }

    fn add_ringing(
        &self,
        timer: TimerId,
        at: Timestamp,
        infos: &HashMap<TimerId, TimerInfo>,
        while_closed: bool,
    ) {
        let (name, task) = infos.get(&timer).map_or_else(
            || ("Temporizador".to_owned(), None),
            |info| (info.name.clone(), info.task_id),
        );
        self.state.borrow_mut().ringing.push(Ringing {
            timer,
            name,
            task,
            at,
            while_closed,
        });
    }

    /// The name and task of `timer`: from the snapshot taken before the tick, else from the
    /// timer itself (a pomodoro stays in the set).
    fn info(&self, timer: TimerId, infos: &HashMap<TimerId, TimerInfo>) -> TimerInfo {
        if let Some(info) = infos.get(&timer) {
            return info.clone();
        }
        let handle = self.core.borrow();
        let tracker = handle.tracker();
        tracker.timers().get(timer).map_or_else(
            || TimerInfo {
                name: "Timer".to_owned(),
                task: None,
                task_id: None,
            },
            |active| Names::load(tracker.store(), active.task).info(active),
        )
    }

    /// The events of the reopen tick: finished countdowns ring as "terminou às HH:MM" and the
    /// notice says what else ended while the app was closed. No sound for those.
    fn reopened(&self, events: &[TrackerEvent], before: &[ActiveTimer]) {
        if events.is_empty() {
            return;
        }
        let infos = {
            let handle = self.core.borrow();
            infos_of(&handle, &before.iter().collect::<Vec<_>>())
        };
        for event in events {
            if let TrackerEvent::Timer(TimersEvent::CountdownFinished { timer, at }) = event {
                self.add_ringing(*timer, *at, &infos, true);
            }
        }
        let notices = alerts::reopen_notices(events, &infos, format::local_hh_mm);
        tracing::info!("while closed: {notices:?}");
        self.state.borrow_mut().notice = notices.join("\n");
    }

    /// Shows the forgotten dialog for a running stopwatch past the limit, once per stopwatch.
    fn check_forgotten(&self) -> Option<PanelKind> {
        let found = {
            let handle = self.core.borrow();
            handle.tracker().forgotten(handle.now())
        };
        let mut state = self.state.borrow_mut();
        let ids = match found {
            Ok(ids) => {
                state.forgotten_failing = false;
                ids
            }
            Err(e) => {
                // Retried every second: log once per failure streak.
                if !std::mem::replace(&mut state.forgotten_failing, true) {
                    tracing::warn!("cannot check forgotten stopwatches (retrying): {e}");
                }
                return None;
            }
        };
        if state.forgotten.is_some_and(|id| !ids.contains(&id)) {
            state.forgotten = None;
        }
        if state.forgotten.is_some() {
            return None;
        }
        let id = ids
            .into_iter()
            .find(|id| !state.forgotten_asked.contains(id))?;
        state.forgotten_asked.insert(id);
        state.forgotten = Some(id);
        state.stopwatch = Some(id);
        state.view = View::Stopwatch;
        drop(state);
        // The window may be hidden (the tick renders only a visible one): fill it now, so it
        // shows the dialog and not a stale view when it opens.
        self.render();
        self.interactive.then_some(PanelKind::Stopwatch)
    }

    /// Rebuilds the visible view from the tracker and pushes it to the window.
    pub fn render(&self) {
        let Some(window) = self.ui.upgrade() else {
            return;
        };
        self.forget_deleted_tasks();
        // M7a: read before the core is borrowed below.
        let alarm_cards = self.alarms.cards();
        let alarm_title = self.alarms.title();
        let frame = {
            let handle = self.core.borrow();
            let tracker = handle.tracker();
            let mut state = self.state.borrow_mut();
            state::normalize(&mut state, tracker.timers());
            let tasks = linked_tasks(tracker, &state).into_iter();
            let names = Names::load(
                tracker.store(),
                tasks.chain(alarm_cards.iter().filter_map(|c| c.task)),
            );
            frame::build(&frame::Source {
                tracker,
                state: &state,
                names: &names,
                now: handle.now(),
                alarm_cards: &alarm_cards,
                alarm_title,
            })
        };
        apply::apply(&window, &self.models, frame);
        if window.window().is_visible() {
            let mut state = self.state.borrow_mut();
            state.notice_seen = !state.notice.is_empty();
        }
    }
}

impl TimerPanels {
    /// Clears the drafts whose task was deleted (in Gerenciar, or by any other path), so the
    /// next start does not fail on it. Runs before every render and every start.
    pub(super) fn forget_deleted_tasks(&self) {
        let tasks = state::draft_tasks(&self.state.borrow());
        if tasks.is_empty() {
            return;
        }
        let missing: Vec<TaskId> = {
            let core = self.core.borrow();
            let store = core.tracker().store();
            tasks
                .into_iter()
                .filter(|&t| matches!(store.task(t), Err(StoreError::NotFound { .. })))
                .collect()
        };
        if !missing.is_empty() {
            tracing::info!("dropping deleted tasks from the timer drafts: {missing:?}");
            state::forget_missing_tasks(&mut self.state.borrow_mut(), |t| !missing.contains(&t));
        }
    }
}

/// The tasks a frame may show: timers, ringing countdowns, drafts and the wizard.
fn linked_tasks(
    tracker: &clocky_core::tracker::Tracker,
    state: &State,
) -> Vec<clocky_core::domain::TaskId> {
    tracker
        .timers()
        .iter()
        .filter_map(|a| a.task)
        .chain(state.ringing.iter().filter_map(|r| r.task))
        .chain(
            [
                state.stopwatch_task,
                state.countdown_task,
                state.pomodoro_task,
                state.wizard.task,
            ]
            .into_iter()
            .flatten(),
        )
        .collect()
}

/// Names of the timers that come due at `now` (a finished countdown leaves the set).
fn infos_of_due(handle: &AppCore, now: Timestamp) -> HashMap<TimerId, TimerInfo> {
    let due: Vec<&ActiveTimer> = handle
        .tracker()
        .timers()
        .iter()
        .filter(|active| is_due(active, now))
        .collect();
    infos_of(handle, &due)
}

fn infos_of(handle: &AppCore, timers: &[&ActiveTimer]) -> HashMap<TimerId, TimerInfo> {
    if timers.is_empty() {
        return HashMap::new();
    }
    let names = Names::load(
        handle.tracker().store(),
        timers.iter().filter_map(|a| a.task),
    );
    timers.iter().map(|a| (a.id(), names.info(a))).collect()
}

fn is_due(active: &ActiveTimer, now: Timestamp) -> bool {
    match &active.timer {
        Timer::Countdown(cd) => cd.finished_at(now).is_some(),
        Timer::Pomodoro(p) => p.is_running() && p.remaining(now).is_zero(),
        Timer::Stopwatch(_) => false,
    }
}

/// The kind of an active timer, for opening its panel.
fn kind_of(handle: &AppCore, id: TimerId) -> Option<Kind> {
    handle
        .tracker()
        .timers()
        .get(id)
        .map(|active| Kind::of(&active.timer))
}
