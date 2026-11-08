//! What the panel buttons do. Each action takes short borrows, runs one tracker command and
//! re-renders; nothing here holds a borrow while Slint runs.

use clocky_core::domain::{SubtaskId, TaskId, TimerId, TimerPresetId};
use clocky_core::engine::timers::{Timer, TimersEvent};
use clocky_core::time::Timestamp;
use clocky_core::tracker::{Tracker, TrackerError, TrackerEvent};

use super::cards::Kind;
use super::hms::Field;
use super::state::{PomodoroPick, View, pick, pick_countdown};
use super::{TimerPanels, format};

/// Shown when a command fails (the details go to the log).
const FAILED: &str = "Não deu para concluir a ação. Os detalhes estão no log.";

/// Commands.
impl TimerPanels {
    /// Runs a tracker command at the current instant. A failure is logged and noticed.
    pub(super) fn command<T>(
        &self,
        what: &str,
        f: impl FnOnce(&mut Tracker, Timestamp) -> Result<T, TrackerError>,
    ) -> Option<T> {
        let result = {
            let mut handle = self.core.borrow_mut();
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

    pub(super) fn picked(&self, kind: Kind) -> Option<TimerId> {
        let handle = self.core.borrow();
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
            let handle = self.core.borrow();
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
        // The ready stopwatch keeps the task of the session that just ended (screen 32).
        let stopwatch_task = {
            let handle = self.core.borrow();
            handle
                .tracker()
                .timers()
                .get(id)
                .and_then(|a| match a.timer {
                    Timer::Stopwatch(_) => Some(a.task),
                    _ => None,
                })
        };
        if let Some(task) = stopwatch_task {
            self.state.borrow_mut().stopwatch_task = task;
        }
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
        if let Some(id) = self.picked(Kind::Stopwatch) {
            self.stop(id);
        }
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
                .core
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
        let handle = self.core.borrow();
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
            let handle = self.core.borrow();
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
