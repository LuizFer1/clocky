//! Ativos cards, the forgotten dialog and the task chooser.

use clocky_core::domain::{TaskId, TimerId};
use clocky_core::engine::timers::Timers;

use super::cards::Kind;
use super::state::{PomodoroPick, View};
use super::{TimerPanels, kind_of};

/// Where a chosen task goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Destination {
    /// The timer the chooser was opened for, still active.
    Timer(TimerId),
    /// The chooser was opened for a draft (nothing running yet).
    Draft,
    /// That timer ended meanwhile: the choice must not land on another one.
    Dropped,
}

/// The destination of a choice made for `captured` (the timer shown when "Trocar" was clicked;
/// `None` = a draft), given the timers active now.
pub(super) fn destination(captured: Option<TimerId>, timers: &Timers) -> Destination {
    match captured {
        None => Destination::Draft,
        Some(id) if timers.get(id).is_some() => Destination::Timer(id),
        Some(_) => Destination::Dropped,
    }
}

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
        let kind = kind_of(&self.core.borrow(), id);
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

    /// "Vincular tarefa" / "Trocar": opens the picker for the timer shown now. The choice goes
    /// to that timer only while it is still active (see [`destination`]).
    pub(super) fn choose_task(&self, target: TaskTarget) {
        let timer = self.target_timer(target);
        let current = self.current_task(target);
        let weak = self.weak.clone();
        self.chooser.choose(
            current,
            Box::new(move |task| {
                if let Some(panels) = weak.upgrade() {
                    panels.set_task(target, timer, task);
                }
            }),
        );
    }

    fn current_task(&self, target: TaskTarget) -> Option<TaskId> {
        let timer = self.target_timer(target);
        let handle = self.core.borrow();
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

    /// Applies a chosen task: to the whole session of the timer it was chosen for, else to the
    /// draft. If that timer ended meanwhile, the choice is dropped.
    fn set_task(&self, target: TaskTarget, timer: Option<TimerId>, task: Option<TaskId>) {
        let destination = destination(timer, self.core.borrow().tracker().timers());
        match destination {
            Destination::Timer(id) => {
                self.command("set task", |t, _| t.set_task(id, task));
            }
            Destination::Draft => {
                let mut state = self.state.borrow_mut();
                match target {
                    TaskTarget::Stopwatch => state.stopwatch_task = task,
                    TaskTarget::Countdown | TaskTarget::CountdownRun => {
                        state.countdown_task = task;
                    }
                    TaskTarget::Pomodoro => state.pomodoro_task = task,
                    TaskTarget::Wizard => state.wizard.task = task,
                }
            }
            Destination::Dropped => {
                tracing::info!("task choice dropped: timer {:?} is no longer active", timer);
            }
        }
        self.render();
    }
}

#[cfg(test)]
mod tests {
    use clocky_core::clock::FakeClock;
    use clocky_core::time::Timestamp;

    use super::*;
    use crate::core::AppCore;

    #[test]
    fn a_choice_lands_only_on_the_timer_it_was_made_for() {
        let clock = FakeClock::new(Timestamp::from_millis(1_000));
        let mut core = AppCore::in_memory(Box::new(clock));
        let now = core.now();
        let first = core.tracker_mut().start_stopwatch(None, now).unwrap();
        assert_eq!(
            destination(Some(first), core.tracker().timers()),
            Destination::Timer(first)
        );
        // "Trocar" on the first stopwatch, then it stops and another one starts.
        core.tracker_mut().discard(first).unwrap();
        let second = core.tracker_mut().start_stopwatch(None, now).unwrap();
        assert_eq!(
            destination(Some(first), core.tracker().timers()),
            Destination::Dropped
        );
        assert_eq!(
            destination(Some(second), core.tracker().timers()),
            Destination::Timer(second)
        );
        assert_eq!(
            destination(None, core.tracker().timers()),
            Destination::Draft
        );
    }
}
