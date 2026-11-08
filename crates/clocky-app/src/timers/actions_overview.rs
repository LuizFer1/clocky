//! Ativos cards, the forgotten dialog and the task chooser.

use clocky_core::domain::{TaskId, TimerId};

use super::cards::Kind;
use super::state::{PomodoroPick, View};
use super::{TimerPanels, kind_of};

/// Whose task the chooser changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTarget {
    Stopwatch,
    Countdown,
    CountdownRun,
    Pomodoro,
    Wizard,
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
