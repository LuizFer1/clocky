//! What the panel buttons do. Each action takes short borrows, runs one tracker command and
//! re-renders; nothing here holds a borrow while Slint runs.

use clocky_core::domain::{PomodoroPresetId, SubtaskId, TaskId, TimerId, TimerPresetId};
use clocky_core::engine::timers::{Timer, TimersEvent};
use clocky_core::time::Timestamp;
use clocky_core::tracker::{Tracker, TrackerError, TrackerEvent};

use super::cards::Kind;
use super::frame_pomodoro::preset_config;
use super::hms::Field;
use super::state::{PomodoroPick, View, pick, pick_countdown};
use super::wizard::{Defaults, Wizard};
use super::{TimerPanels, format, kind_of};

/// Shown when a command fails (the details go to the log).
const FAILED: &str = "Não deu para concluir a ação. Os detalhes estão no log.";

/// Store keys of the wizard defaults (decision 13: the settings only seed the wizard).
const DEFAULT_KEYS: [&str; 5] = [
    "pomodoro_focus_min",
    "pomodoro_short_min",
    "pomodoro_long_min",
    "pomodoro_rounds",
    "pomodoro_auto_start",
];

/// Whose task the chooser changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTarget {
    Stopwatch,
    Countdown,
    CountdownRun,
    Pomodoro,
    Wizard,
}

/// Commands.
impl TimerPanels {
    /// Runs a tracker command at the current instant. A failure is logged and noticed.
    fn command<T>(
        &self,
        what: &str,
        f: impl FnOnce(&mut Tracker, Timestamp) -> Result<T, TrackerError>,
    ) -> Option<T> {
        let result = {
            let mut handle = self.handle.borrow_mut();
            let now = handle.now();
            f(handle.tracker_mut(), now)
        };
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::warn!("{what} failed: {e}");
                FAILED.clone_into(&mut self.state.borrow_mut().notice);
                None
            }
        }
    }

    fn picked(&self, kind: Kind) -> Option<TimerId> {
        let handle = self.handle.borrow();
        let state = self.state.borrow();
        let selected = match kind {
            Kind::Stopwatch => state.stopwatch,
            Kind::Countdown => state.countdown,
            Kind::Pomodoro => match state.pomodoro {
                PomodoroPick::Active(selected) => selected,
                PomodoroPick::Draft(_) => return None,
            },
        };
        pick(handle.tracker().timers(), kind, selected)
    }

    /// Pause, resume or start the waiting phase, depending on the timer's state.
    pub(super) fn toggle(&self, id: TimerId) {
        let action = {
            let handle = self.handle.borrow();
            handle.tracker().timers().get(id).map(|a| match &a.timer {
                Timer::Pomodoro(p) if p.is_waiting() => Toggle::StartPhase,
                Timer::Pomodoro(p) if p.is_running() => Toggle::Pause,
                Timer::Countdown(cd) if cd.is_running() => Toggle::Pause,
                Timer::Stopwatch(sw) if sw.is_running() => Toggle::Pause,
                _ => Toggle::Resume,
            })
        };
        match action {
            Some(Toggle::StartPhase) => {
                self.command("start phase", |t, now| t.start_phase(id, now))
            }
            Some(Toggle::Pause) => self.command("pause", |t, now| t.pause(id, now)),
            Some(Toggle::Resume) => self.command("resume", |t, now| t.resume(id, now)),
            None => None,
        };
        self.render();
    }

    /// Stops a timer; a stopwatch session gets the "Sessão de … registrada" notice.
    pub(super) fn stop(&self, id: TimerId) {
        let Some(events) = self.command("stop", |t, now| t.stop(id, now)) else {
            self.render();
            return;
        };
        self.notice_recorded(&events);
        self.render();
    }

    fn notice_recorded(&self, events: &[TrackerEvent]) {
        let recorded = events.iter().find_map(|event| match event {
            TrackerEvent::Timer(TimersEvent::SessionCompleted {
                session,
                round: None,
                ..
            }) => Some(session.effective()),
            _ => None,
        });
        if let Some(effective) = recorded {
            self.state.borrow_mut().notice = format::session_recorded(effective);
        }
    }

    pub(super) fn dismiss_notice(&self) {
        self.state.borrow_mut().notice.clear();
        self.render();
    }
}

enum Toggle {
    StartPhase,
    Pause,
    Resume,
}

/// Cronômetro.
impl TimerPanels {
    pub(super) fn stopwatch_primary(&self) {
        if let Some(id) = self.picked(Kind::Stopwatch) {
            self.toggle(id);
            return;
        }
        let task = self.state.borrow().stopwatch_task;
        if let Some(id) = self.command("start stopwatch", |t, now| t.start_stopwatch(task, now)) {
            let mut state = self.state.borrow_mut();
            state.stopwatch = Some(id);
            state.notice.clear();
        }
        self.render();
    }

    pub(super) fn stopwatch_stop(&self) {
        let Some(id) = self.picked(Kind::Stopwatch) else {
            return;
        };
        // The ready view keeps the task of the session that just ended (screen 32).
        let task = self
            .handle
            .borrow()
            .tracker()
            .timers()
            .get(id)
            .and_then(|a| a.task);
        self.state.borrow_mut().stopwatch_task = task;
        self.stop(id);
    }

    /// A checklist click: linked to the running stopwatch's session, if any.
    pub(super) fn toggle_subtask(&self, id: i32, done: bool) {
        let Some(sub) = super::convert::row_from_ui(id).map(SubtaskId) else {
            return;
        };
        let run = self.picked(Kind::Stopwatch);
        if done {
            self.command("complete subtask", |t, now| {
                t.complete_subtask(sub, now, run)
            });
        } else {
            self.command("reopen subtask", |t, now| t.reopen_subtask(sub, now, run));
        }
        self.render();
    }

    pub(super) fn add_subtask(&self, name: &str) {
        let name = name.trim();
        let task = match self.picked(Kind::Stopwatch) {
            Some(id) => self
                .handle
                .borrow()
                .tracker()
                .timers()
                .get(id)
                .and_then(|a| a.task),
            None => self.state.borrow().stopwatch_task,
        };
        let (Some(task), false) = (task, name.is_empty()) else {
            return;
        };
        self.command("add subtask", |t, _| {
            Ok(t.store_mut().add_subtask(task, name)?)
        });
        self.render();
    }
}

/// Temporizador.
impl TimerPanels {
    pub(super) fn countdown_step(&self, field: i32, up: bool) {
        if let Some(field) = Field::from_index(field) {
            let mut state = self.state.borrow_mut();
            state.hms = state.hms.step(field, up);
        }
        self.render();
    }

    pub(super) fn countdown_add_minutes(&self, minutes: i32) {
        {
            let mut state = self.state.borrow_mut();
            state.hms = state.hms.add_minutes(u32::try_from(minutes).unwrap_or(0));
        }
        self.render();
    }

    pub(super) fn countdown_start(&self) {
        let (task, hms) = {
            let state = self.state.borrow();
            (state.countdown_task, state.hms)
        };
        if hms.is_zero() {
            return;
        }
        let target = hms.duration();
        let started = self.command("start countdown", |t, now| {
            t.start_countdown(task, target, None, now)
        });
        self.countdown_started(started);
    }

    pub(super) fn countdown_start_preset(&self, id: i32) {
        let Some(id) = super::convert::row_from_ui(id).map(TimerPresetId) else {
            return;
        };
        let preset = self.timer_preset(id);
        let Some((duration, task)) = preset else {
            return;
        };
        let started = self.command("start countdown preset", |t, now| {
            t.start_countdown(task, duration, Some(id), now)
        });
        self.countdown_started(started);
    }

    fn timer_preset(&self, id: TimerPresetId) -> Option<(std::time::Duration, Option<TaskId>)> {
        let handle = self.handle.borrow();
        let presets = handle.tracker().store().timer_presets();
        presets
            .inspect_err(|e| tracing::warn!("cannot read the timer presets: {e}"))
            .ok()?
            .into_iter()
            .find(|p| p.id == id)
            .map(|p| (p.duration, p.task))
    }

    fn countdown_started(&self, started: Option<TimerId>) {
        if let Some(id) = started {
            let mut state = self.state.borrow_mut();
            state.countdown = Some(id);
            state.view = View::CountdownRun;
        }
        self.render();
    }

    /// Clicking a preset row loads its duration and task into the editor.
    pub(super) fn countdown_load_preset(&self, id: i32) {
        let preset = super::convert::row_from_ui(id)
            .map(TimerPresetId)
            .and_then(|id| self.timer_preset(id));
        if let Some((duration, task)) = preset {
            let mut state = self.state.borrow_mut();
            state.hms = super::hms::Hms::from_secs(duration.as_secs());
            state.countdown_task = task;
        }
        self.render();
    }

    /// "+ Novo preset": saves the current values under `name`.
    pub(super) fn countdown_save_preset(&self, name: &str) {
        let (hms, task) = {
            let state = self.state.borrow();
            (state.hms, state.countdown_task)
        };
        if hms.is_zero() {
            return;
        }
        let name = match name.trim() {
            "" => format!("Temporizador {}", format::duration_short(hms.duration())),
            name => name.to_owned(),
        };
        self.command("save timer preset", |t, _| {
            Ok(t.store_mut()
                .create_timer_preset(&name, hms.duration(), task)?)
        });
        self.render();
    }

    pub(super) fn countdown_run_primary(&self) {
        if let Some(id) = self.picked(Kind::Countdown) {
            self.toggle(id);
        }
    }

    pub(super) fn countdown_cancel(&self) {
        if let Some(id) = self.picked(Kind::Countdown) {
            self.stop(id);
        }
    }

    pub(super) fn dismiss(&self, id: Option<TimerId>) {
        {
            let mut state = self.state.borrow_mut();
            let handle = self.handle.borrow();
            let target = id.or_else(|| {
                pick_countdown(handle.tracker().timers(), &state.ringing, state.countdown)
            });
            if let Some(target) = target {
                state.ringing.retain(|r| r.timer != target);
            }
        }
        self.render();
    }
}

/// Pomodoro.
impl TimerPanels {
    pub(super) fn pomodoro_open_preset(&self, id: i32) {
        let Some(id) = super::convert::row_from_ui(id).map(PomodoroPresetId) else {
            return;
        };
        let task = {
            let handle = self.handle.borrow();
            let presets = handle
                .tracker()
                .store()
                .pomodoro_presets()
                .unwrap_or_default();
            presets
                .into_iter()
                .find(|p| p.id == id)
                .and_then(|p| p.task)
        };
        self.open_draft(id, task);
    }

    fn open_draft(&self, id: PomodoroPresetId, task: Option<TaskId>) {
        {
            let mut state = self.state.borrow_mut();
            state.pomodoro = PomodoroPick::Draft(id);
            state.pomodoro_task = task;
            state.view = View::PomodoroTimer;
        }
        self.render();
    }

    pub(super) fn pomodoro_primary(&self) {
        let pick = self.state.borrow().pomodoro;
        match pick {
            PomodoroPick::Draft(id) => self.pomodoro_start(id),
            PomodoroPick::Active(_) => {
                if let Some(id) = self.picked(Kind::Pomodoro) {
                    self.toggle(id);
                }
            }
        }
    }

    fn pomodoro_start(&self, id: PomodoroPresetId) {
        let config = {
            let handle = self.handle.borrow();
            let presets = handle
                .tracker()
                .store()
                .pomodoro_presets()
                .unwrap_or_default();
            presets.iter().find(|p| p.id == id).and_then(preset_config)
        };
        let Some(config) = config else {
            return;
        };
        let task = self.state.borrow().pomodoro_task;
        let started = self.command("start pomodoro", |t, now| {
            t.start_pomodoro(task, config, Some(id), now)
        });
        if let Some(timer) = started {
            self.state.borrow_mut().pomodoro = PomodoroPick::Active(Some(timer));
        }
        self.render();
    }

    pub(super) fn pomodoro_stop(&self) {
        if let Some(id) = self.picked(Kind::Pomodoro) {
            self.stop(id);
        }
    }

    pub(super) fn pomodoro_skip(&self) {
        if let Some(id) = self.picked(Kind::Pomodoro) {
            self.command("skip phase", |t, now| t.skip(id, now));
            self.render();
        }
    }
}

/// Novo pomodoro.
impl TimerPanels {
    /// Reads the wizard defaults from the store settings (constants when unset or invalid).
    pub(super) fn load_wizard_defaults(&self) {
        let values: Vec<Option<String>> = {
            let handle = self.handle.borrow();
            let store = handle.tracker().store();
            DEFAULT_KEYS
                .iter()
                .map(|key| store.setting(key).ok().flatten())
                .collect()
        };
        let base = Defaults::default();
        let num = |i: usize, fallback: u32| {
            values[i]
                .as_deref()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(fallback)
        };
        let defaults = Defaults {
            focus: num(0, base.focus),
            short: num(1, base.short),
            long: num(2, base.long),
            rounds: num(3, base.rounds),
            auto_start: values[4]
                .as_deref()
                .map_or(base.auto_start, |v| v == "true" || v == "1"),
        };
        let mut state = self.state.borrow_mut();
        state.wizard = Wizard::new(defaults);
        state.wizard_defaults = defaults;
    }

    pub(super) fn wizard_open(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.wizard = Wizard::new(state.wizard_defaults);
            state.view = View::Wizard;
        }
        self.render();
    }

    pub(super) fn wizard_edit(&self, edit: impl FnOnce(&mut Wizard)) {
        edit(&mut self.state.borrow_mut().wizard);
        self.render();
    }

    pub(super) fn wizard_back(&self) {
        let moved = self.state.borrow_mut().wizard.back();
        if moved {
            self.render();
        } else {
            self.set_view(View::PomodoroList);
        }
    }

    /// "Próximo", or "Criar pomodoro" on the last step: saves the preset and opens its timer.
    pub(super) fn wizard_next(&self) {
        let moved = self.state.borrow_mut().wizard.next();
        if moved {
            self.render();
            return;
        }
        let wizard = self.state.borrow().wizard.clone();
        let name = wizard.final_name();
        let created = self.command("create pomodoro preset", |t, _| {
            Ok(t.store_mut()
                .create_pomodoro_preset(&wizard.preset(&name))?)
        });
        match created {
            Some(id) => self.open_draft(id, wizard.task),
            None => self.render(),
        }
    }
}

/// Ativos, the forgotten dialog and the task chooser.
impl TimerPanels {
    /// A card click opens that timer's panel.
    pub(super) fn open_card(&self, id: TimerId) {
        let kind = kind_of(&self.handle.borrow(), id);
        {
            let mut state = self.state.borrow_mut();
            let ringing = state.ringing(id).is_some();
            match (kind, ringing) {
                (_, true) | (Some(Kind::Countdown), _) => {
                    state.countdown = Some(id);
                    state.view = View::CountdownRun;
                }
                (Some(Kind::Stopwatch), _) => {
                    state.stopwatch = Some(id);
                    state.view = View::Stopwatch;
                }
                (Some(Kind::Pomodoro), _) => {
                    state.pomodoro = PomodoroPick::Active(Some(id));
                    state.view = View::PomodoroTimer;
                }
                (None, false) => {}
            }
        }
        self.render();
    }

    pub(super) fn forgotten_answer(&self, record: bool) {
        let Some(id) = self.state.borrow_mut().forgotten.take() else {
            return;
        };
        if record {
            self.stop(id);
        } else {
            self.command("discard stopwatch", |t, _| t.discard(id));
            "Sessão descartada".clone_into(&mut self.state.borrow_mut().notice);
            self.render();
        }
    }

    pub(super) fn choose_task(&self, target: TaskTarget) {
        let current = self.current_task(target);
        let weak = self.weak.clone();
        self.chooser.choose(
            current,
            Box::new(move |task| {
                if let Some(panels) = weak.upgrade() {
                    panels.set_task(target, task);
                }
            }),
        );
    }

    fn current_task(&self, target: TaskTarget) -> Option<TaskId> {
        let timer = self.target_timer(target);
        let handle = self.handle.borrow();
        if let Some(active) = timer.and_then(|id| handle.tracker().timers().get(id)) {
            return active.task;
        }
        let state = self.state.borrow();
        match target {
            TaskTarget::Stopwatch => state.stopwatch_task,
            TaskTarget::Countdown | TaskTarget::CountdownRun => state.countdown_task,
            TaskTarget::Pomodoro => state.pomodoro_task,
            TaskTarget::Wizard => state.wizard.task,
        }
    }

    /// The active timer a target changes; `None` = a draft.
    fn target_timer(&self, target: TaskTarget) -> Option<TimerId> {
        match target {
            TaskTarget::Stopwatch => self.picked(Kind::Stopwatch),
            TaskTarget::CountdownRun => self.picked(Kind::Countdown),
            TaskTarget::Pomodoro => self.picked(Kind::Pomodoro),
            TaskTarget::Countdown | TaskTarget::Wizard => None,
        }
    }

    /// Applies a chosen task: to the whole session of a running timer, else to the draft.
    fn set_task(&self, target: TaskTarget, task: Option<TaskId>) {
        if let Some(id) = self.target_timer(target) {
            self.command("set task", |t, _| t.set_task(id, task));
        } else {
            let mut state = self.state.borrow_mut();
            match target {
                TaskTarget::Stopwatch => state.stopwatch_task = task,
                TaskTarget::Countdown | TaskTarget::CountdownRun => state.countdown_task = task,
                TaskTarget::Pomodoro => state.pomodoro_task = task,
                TaskTarget::Wizard => state.wizard.task = task,
            }
        }
        self.render();
    }
}
