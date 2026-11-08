//! Set of active timers: several of any kind at the same time.

use std::collections::BTreeMap;

use super::countdown::Countdown;
use super::error::EngineError;
use super::pomodoro::{Phase, Pomodoro, PomodoroEvent};
use super::session::CompletedSession;
use super::stopwatch::Stopwatch;
use crate::domain::{PresetRef, TaskId, TimerId};
use crate::time::Timestamp;

/// A timer of any kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timer {
    /// Pomodoro.
    Pomodoro(Pomodoro),
    /// Stopwatch.
    Stopwatch(Stopwatch),
    /// Countdown timer.
    Countdown(Countdown),
}

impl Timer {
    /// Pauses the timer.
    ///
    /// # Errors
    /// [`EngineError::NotRunning`] if it is already paused (or is a waiting pomodoro phase).
    pub fn pause(&mut self, now: Timestamp) -> Result<(), EngineError> {
        match self {
            Self::Pomodoro(p) => p.pause(now),
            Self::Stopwatch(sw) => sw.pause(now),
            Self::Countdown(cd) => cd.pause(now),
        }
    }

    /// Resumes the timer.
    ///
    /// # Errors
    /// [`EngineError::AlreadyRunning`] if it is running; [`EngineError::NotRunning`] if it is a
    /// waiting pomodoro phase (use [`Timers::start_phase`]).
    pub fn resume(&mut self, now: Timestamp) -> Result<(), EngineError> {
        match self {
            Self::Pomodoro(p) => p.resume(now),
            Self::Stopwatch(sw) => sw.resume(now),
            Self::Countdown(cd) => cd.resume(now),
        }
    }
}

/// Active timer with its linked task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTimer {
    /// Identifier (read-only, so it cannot diverge from the key in the set).
    id: TimerId,
    /// Linked task (can be changed while the timer is running).
    pub task: Option<TaskId>,
    /// Preset the timer was started from, if any.
    pub preset: Option<PresetRef>,
    /// The timer.
    pub timer: Timer,
}

impl ActiveTimer {
    /// Rebuilds an active timer with a known identifier, to restore it with [`Timers::restore`].
    #[must_use]
    pub fn new(id: TimerId, task: Option<TaskId>, preset: Option<PresetRef>, timer: Timer) -> Self {
        Self {
            id,
            task,
            preset,
            timer,
        }
    }

    /// Identifier of the timer.
    #[must_use]
    pub fn id(&self) -> TimerId {
        self.id
    }
}

/// Event produced by the set of timers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimersEvent {
    /// A session ended and must be recorded.
    SessionCompleted {
        /// Timer that produced the session.
        timer: TimerId,
        /// Task linked at the moment the session ended.
        task: Option<TaskId>,
        /// The session.
        session: CompletedSession,
        /// Round of the pomodoro (`None` for the other kinds).
        round: Option<u32>,
    },
    /// A pomodoro phase ended.
    PomodoroPhaseEnded {
        /// Pomodoro.
        timer: TimerId,
        /// Phase that ended.
        ended: Phase,
        /// Next phase.
        next: Phase,
        /// Did the next phase start on its own?
        auto_started: bool,
    },
    /// A countdown came due (and left the set).
    CountdownFinished {
        /// Countdown.
        timer: TimerId,
        /// Exact instant at which it came due.
        at: Timestamp,
    },
}

/// Set of active timers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timers {
    next_id: u64,
    active: BTreeMap<TimerId, ActiveTimer>,
}

impl Timers {
    /// Empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a timer and returns its identifier.
    pub fn add(&mut self, task: Option<TaskId>, timer: Timer) -> TimerId {
        self.next_id += 1;
        let id = TimerId(self.next_id);
        self.active
            .insert(id, ActiveTimer::new(id, task, None, timer));
        id
    }

    /// Restores a set of timers; `next_id` is the identifier the next [`Timers::add`] assigns.
    ///
    /// # Errors
    /// [`EngineError::InvalidTimerId`] if two timers share an identifier or one is not below
    /// `next_id`.
    pub fn restore(next_id: u64, timers: Vec<ActiveTimer>) -> Result<Self, EngineError> {
        let mut active = BTreeMap::new();
        for timer in timers {
            let id = timer.id;
            if id.0 >= next_id || active.insert(id, timer).is_some() {
                return Err(EngineError::InvalidTimerId(id));
            }
        }
        Ok(Self {
            next_id: next_id.saturating_sub(1),
            active,
        })
    }

    /// Removes a timer without recording anything ("Discard" in the forgotten dialog).
    ///
    /// # Errors
    /// [`EngineError::TimerNotFound`] if the timer does not exist.
    pub fn discard(&mut self, id: TimerId) -> Result<ActiveTimer, EngineError> {
        self.active
            .remove(&id)
            .ok_or(EngineError::TimerNotFound(id))
    }

    /// Pauses a timer.
    ///
    /// # Errors
    /// [`EngineError::TimerNotFound`] if the timer does not exist; otherwise as [`Timer::pause`].
    pub fn pause(&mut self, id: TimerId, now: Timestamp) -> Result<(), EngineError> {
        self.timer_mut(id)?.pause(now)
    }

    /// Resumes a timer.
    ///
    /// # Errors
    /// [`EngineError::TimerNotFound`] if the timer does not exist; otherwise as
    /// [`Timer::resume`].
    pub fn resume(&mut self, id: TimerId, now: Timestamp) -> Result<(), EngineError> {
        self.timer_mut(id)?.resume(now)
    }

    /// Starts the waiting phase of a pomodoro.
    ///
    /// # Errors
    /// [`EngineError::TimerNotFound`] if the timer does not exist; [`EngineError::NotPomodoro`]
    /// if it is not a pomodoro; [`EngineError::AlreadyRunning`] if the phase has started.
    pub fn start_phase(&mut self, id: TimerId, now: Timestamp) -> Result<(), EngineError> {
        match self.timer_mut(id)? {
            Timer::Pomodoro(p) => p.start_phase(now),
            _ => Err(EngineError::NotPomodoro(id)),
        }
    }

    fn timer_mut(&mut self, id: TimerId) -> Result<&mut Timer, EngineError> {
        self.active
            .get_mut(&id)
            .map(|active| &mut active.timer)
            .ok_or(EngineError::TimerNotFound(id))
    }

    /// Active timer with this identifier.
    #[must_use]
    pub fn get(&self, id: TimerId) -> Option<&ActiveTimer> {
        self.active.get(&id)
    }

    /// Active timer with this identifier, to control it (pause, resume, ...).
    pub fn get_mut(&mut self, id: TimerId) -> Option<&mut ActiveTimer> {
        self.active.get_mut(&id)
    }

    /// Active timers, in creation order.
    pub fn iter(&self) -> impl Iterator<Item = &ActiveTimer> {
        self.active.values()
    }

    /// Changes the linked task (applies to the whole session).
    ///
    /// # Errors
    /// [`EngineError::TimerNotFound`] if the timer does not exist.
    pub fn set_task(&mut self, id: TimerId, task: Option<TaskId>) -> Result<(), EngineError> {
        let active = self
            .active
            .get_mut(&id)
            .ok_or(EngineError::TimerNotFound(id))?;
        active.task = task;
        Ok(())
    }

    /// Stops and removes a timer. Returns the session to record, if any.
    ///
    /// # Errors
    /// [`EngineError::TimerNotFound`] if the timer does not exist.
    pub fn stop(
        &mut self,
        id: TimerId,
        now: Timestamp,
    ) -> Result<Option<TimersEvent>, EngineError> {
        let active = self
            .active
            .remove(&id)
            .ok_or(EngineError::TimerNotFound(id))?;
        let (session, round) = match active.timer {
            Timer::Pomodoro(p) => {
                let round = p.round();
                (p.stop(now), Some(round))
            }
            Timer::Stopwatch(sw) => (sw.stop(now), None),
            Timer::Countdown(cd) => match cd.finished_at(now) {
                Some(due) => (cd.finish(due), None),
                None => (cd.cancel(now), None),
            },
        };
        Ok(session.map(|session| TimersEvent::SessionCompleted {
            timer: id,
            task: active.task,
            session,
            round,
        }))
    }

    /// Skips the current phase of a pomodoro (an interrupted focus becomes a partial session).
    ///
    /// # Errors
    /// [`EngineError::TimerNotFound`] if the timer does not exist;
    /// [`EngineError::NotPomodoro`] if it is not a pomodoro.
    pub fn skip(&mut self, id: TimerId, now: Timestamp) -> Result<Vec<TimersEvent>, EngineError> {
        let active = self
            .active
            .get_mut(&id)
            .ok_or(EngineError::TimerNotFound(id))?;
        let task = active.task;
        let Timer::Pomodoro(p) = &mut active.timer else {
            return Err(EngineError::NotPomodoro(id));
        };
        Ok(p.skip(now)
            .into_iter()
            .map(|event| from_pomodoro(id, task, event))
            .collect())
    }

    /// Advances all timers up to `now` and returns the events, in creation order.
    #[must_use = "events must be processed (sessions to record)"]
    pub fn tick(&mut self, now: Timestamp) -> Vec<TimersEvent> {
        let ids: Vec<TimerId> = self.active.keys().copied().collect();
        ids.into_iter()
            .flat_map(|id| self.tick_one(id, now))
            .collect()
    }

    fn tick_one(&mut self, id: TimerId, now: Timestamp) -> Vec<TimersEvent> {
        let Some(active) = self.active.get_mut(&id) else {
            return Vec::new();
        };
        let task = active.task;
        match &mut active.timer {
            Timer::Pomodoro(p) => p
                .tick(now)
                .into_iter()
                .map(|event| from_pomodoro(id, task, event))
                .collect(),
            Timer::Countdown(cd) => match cd.finished_at(now) {
                Some(at) => self.finish_countdown(id, at),
                None => Vec::new(),
            },
            Timer::Stopwatch(_) => Vec::new(),
        }
    }

    fn finish_countdown(&mut self, id: TimerId, at: Timestamp) -> Vec<TimersEvent> {
        let Some(ActiveTimer {
            timer: Timer::Countdown(cd),
            task,
            ..
        }) = self.active.remove(&id)
        else {
            return Vec::new();
        };
        let mut events = vec![TimersEvent::CountdownFinished { timer: id, at }];
        if let Some(session) = cd.finish(at) {
            events.push(TimersEvent::SessionCompleted {
                timer: id,
                task,
                session,
                round: None,
            });
        }
        events
    }
}

fn from_pomodoro(timer: TimerId, task: Option<TaskId>, event: PomodoroEvent) -> TimersEvent {
    match event {
        PomodoroEvent::FocusSession { session, round } => TimersEvent::SessionCompleted {
            timer,
            task,
            session,
            round: Some(round),
        },
        PomodoroEvent::PhaseEnded {
            ended,
            next,
            auto_started,
        } => TimersEvent::PomodoroPhaseEnded {
            timer,
            ended,
            next,
            auto_started,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::pomodoro::{Phase, PomodoroConfig};
    use crate::test_util::{at, secs};

    fn stopwatch() -> Timer {
        Timer::Stopwatch(Stopwatch::start(at(0)))
    }

    #[test]
    fn add_assigns_distinct_ids() {
        let mut timers = Timers::new();
        let a = timers.add(None, stopwatch());
        let b = timers.add(None, stopwatch());
        assert_ne!(a, b);
        assert_eq!(timers.iter().count(), 2);
    }

    #[test]
    fn changing_the_task_applies_to_the_whole_session() {
        let mut timers = Timers::new();
        let id = timers.add(None, stopwatch());
        timers.set_task(id, Some(TaskId(7))).unwrap();
        let event = timers.stop(id, at(60)).unwrap().unwrap();
        let TimersEvent::SessionCompleted {
            task,
            session,
            round,
            ..
        } = event
        else {
            panic!("expected a session");
        };
        assert_eq!(
            (task, session.effective(), round),
            (Some(TaskId(7)), secs(60), None)
        );
        assert!(timers.get(id).is_none());
    }

    #[test]
    fn unknown_timer_is_an_error() {
        let mut timers = Timers::new();
        let missing = TimerId(99);
        assert_eq!(
            timers.set_task(missing, None),
            Err(EngineError::TimerNotFound(missing))
        );
        assert_eq!(
            timers.stop(missing, at(0)),
            Err(EngineError::TimerNotFound(missing))
        );
        assert_eq!(
            EngineError::TimerNotFound(missing).to_string(),
            "timer #99 not found"
        );
    }

    #[test]
    fn tick_finishes_countdown_at_its_due_time() {
        let mut timers = Timers::new();
        let id = timers.add(
            Some(TaskId(1)),
            Timer::Countdown(Countdown::start(at(0), secs(60))),
        );
        assert_eq!(timers.tick(at(30)), vec![]);
        let events = timers.tick(at(500));
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            TimersEvent::CountdownFinished {
                timer: id,
                at: at(60)
            }
        );
        let TimersEvent::SessionCompleted { task, session, .. } = &events[1] else {
            panic!("expected a session");
        };
        assert_eq!((*task, session.ended_at()), (Some(TaskId(1)), at(60)));
        assert!(timers.get(id).is_none());
    }

    #[test]
    fn stopping_an_overdue_countdown_ends_at_due_time() {
        let mut timers = Timers::new();
        let id = timers.add(None, Timer::Countdown(Countdown::start(at(0), secs(60))));
        let event = timers.stop(id, at(90)).unwrap().unwrap();
        let TimersEvent::SessionCompleted { session, .. } = event else {
            panic!("expected a session");
        };
        assert_eq!(session.ended_at(), at(60));
    }

    #[test]
    fn pomodoros_run_independently() {
        let mut timers = Timers::new();
        let config = PomodoroConfig::default();
        let a = timers.add(
            Some(TaskId(1)),
            Timer::Pomodoro(Pomodoro::start(config.clone(), at(0))),
        );
        let b = timers.add(
            Some(TaskId(2)),
            Timer::Pomodoro(Pomodoro::start(config, at(600))),
        );
        let events = timers.tick(at(25 * 60));
        assert_eq!(events.len(), 2); // only pomodoro `a` finished its focus
        assert!(matches!(
            events[0],
            TimersEvent::SessionCompleted { timer, task: Some(TaskId(1)), round: Some(1), .. } if timer == a
        ));
        assert!(matches!(
            events[1],
            TimersEvent::PomodoroPhaseEnded {
                timer,
                ended: Phase::Focus,
                next: Phase::ShortBreak,
                auto_started: false,
            } if timer == a
        ));
        let Some(ActiveTimer {
            timer: Timer::Pomodoro(p),
            ..
        }) = timers.get(b)
        else {
            panic!("expected pomodoro b");
        };
        assert_eq!(p.phase(), Phase::Focus);
    }

    #[test]
    fn stopping_a_pomodoro_in_focus_reports_its_round() {
        let mut timers = Timers::new();
        let id = timers.add(
            None,
            Timer::Pomodoro(Pomodoro::start(PomodoroConfig::default(), at(0))),
        );
        let event = timers.stop(id, at(600)).unwrap();
        assert!(matches!(
            event,
            Some(TimersEvent::SessionCompleted { round: Some(1), .. })
        ));
    }

    #[test]
    fn get_mut_allows_controlling_a_timer() {
        let mut timers = Timers::new();
        let id = timers.add(None, stopwatch());
        let Some(ActiveTimer {
            timer: Timer::Stopwatch(sw),
            ..
        }) = timers.get_mut(id)
        else {
            panic!("expected the stopwatch");
        };
        sw.pause(at(10)).unwrap();
        assert!(!sw.is_running());
    }

    #[test]
    fn skip_reports_the_partial_focus_with_its_task() {
        let mut timers = Timers::new();
        let id = timers.add(
            Some(TaskId(3)),
            Timer::Pomodoro(Pomodoro::start(PomodoroConfig::default(), at(0))),
        );
        let events = timers.skip(id, at(600)).unwrap();
        assert_eq!(events.len(), 2);
        let TimersEvent::SessionCompleted {
            timer,
            task,
            session,
            round,
        } = &events[0]
        else {
            panic!("expected a session");
        };
        assert_eq!(
            (*timer, *task, *round, session.effective()),
            (id, Some(TaskId(3)), Some(1), secs(600))
        );
        assert!(matches!(events[1], TimersEvent::PomodoroPhaseEnded { .. }));
    }

    #[test]
    fn skip_on_a_non_pomodoro_is_an_error() {
        let mut timers = Timers::new();
        let id = timers.add(None, stopwatch());
        assert_eq!(timers.skip(id, at(10)), Err(EngineError::NotPomodoro(id)));
        assert_eq!(
            EngineError::NotPomodoro(TimerId(4)).to_string(),
            "timer #4 is not a pomodoro"
        );
        let missing = TimerId(99);
        assert_eq!(
            timers.skip(missing, at(10)),
            Err(EngineError::TimerNotFound(missing))
        );
    }

    #[test]
    fn tick_events_follow_creation_order_across_kinds() {
        let mut timers = Timers::new();
        let cd = timers.add(None, Timer::Countdown(Countdown::start(at(0), secs(60))));
        let config = PomodoroConfig::new(secs(30), secs(10), secs(10), 4, false).unwrap();
        let pomo = timers.add(None, Timer::Pomodoro(Pomodoro::start(config, at(0))));
        let events = timers.tick(at(100));
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            TimersEvent::CountdownFinished {
                timer: cd,
                at: at(60)
            }
        );
        assert!(matches!(
            events[1],
            TimersEvent::SessionCompleted { timer, .. } if timer == cd
        ));
        assert!(matches!(
            events[2],
            TimersEvent::SessionCompleted { timer, .. } if timer == pomo
        ));
        assert!(matches!(
            events[3],
            TimersEvent::PomodoroPhaseEnded { timer, .. } if timer == pomo
        ));
    }

    #[test]
    fn task_changed_mid_focus_is_used_for_the_session() {
        let mut timers = Timers::new();
        let id = timers.add(
            Some(TaskId(1)),
            Timer::Pomodoro(Pomodoro::start(PomodoroConfig::default(), at(0))),
        );
        timers.set_task(id, Some(TaskId(2))).unwrap();
        let events = timers.tick(at(1500));
        assert!(matches!(
            events[0],
            TimersEvent::SessionCompleted {
                task: Some(TaskId(2)),
                ..
            }
        ));
    }

    #[test]
    fn restore_keeps_ids_and_continues_after_next_id() {
        let mut timers = Timers::new();
        let a = timers.add(Some(TaskId(1)), stopwatch());
        let b = timers.add(None, Timer::Countdown(Countdown::start(at(0), secs(60))));
        let parts: Vec<ActiveTimer> = timers.iter().cloned().collect();
        let mut restored = Timers::restore(10, parts).unwrap();
        assert_eq!(restored.get(a), timers.get(a));
        assert_eq!(restored.get(b), timers.get(b));
        assert_eq!(restored.add(None, stopwatch()), TimerId(10));
        assert_eq!(Timers::restore(0, vec![]).unwrap(), Timers::new());
    }

    #[test]
    fn restore_rejects_duplicated_or_stale_ids() {
        let timer = |id| ActiveTimer::new(TimerId(id), None, None, stopwatch());
        assert_eq!(
            Timers::restore(5, vec![timer(1), timer(1)]),
            Err(EngineError::InvalidTimerId(TimerId(1)))
        );
        assert_eq!(
            Timers::restore(5, vec![timer(5)]),
            Err(EngineError::InvalidTimerId(TimerId(5)))
        );
        assert_eq!(
            EngineError::InvalidTimerId(TimerId(5)).to_string(),
            "timer #5 is duplicated or not below the next timer id"
        );
    }

    #[test]
    fn discard_removes_without_a_session() {
        let mut timers = Timers::new();
        let id = timers.add(Some(TaskId(1)), stopwatch());
        let removed = timers.discard(id).unwrap();
        assert_eq!(removed.id(), id);
        assert!(timers.get(id).is_none());
        assert_eq!(timers.discard(id), Err(EngineError::TimerNotFound(id)));
    }

    #[test]
    fn pause_resume_and_start_phase_by_id() {
        let mut timers = Timers::new();
        let config = PomodoroConfig::new(secs(10), secs(10), secs(10), 2, false).unwrap();
        let pomo = timers.add(None, Timer::Pomodoro(Pomodoro::start(config, at(0))));
        let cd = timers.add(None, Timer::Countdown(Countdown::start(at(0), secs(60))));
        let sw = timers.add(None, stopwatch());
        for id in [pomo, cd, sw] {
            timers.pause(id, at(5)).unwrap();
            assert_eq!(timers.pause(id, at(6)), Err(EngineError::NotRunning));
            timers.resume(id, at(7)).unwrap();
            assert_eq!(timers.resume(id, at(8)), Err(EngineError::AlreadyRunning));
        }
        let _events = timers.tick(at(12)); // focus done: the short break waits
        timers.start_phase(pomo, at(12)).unwrap();
        assert_eq!(
            timers.start_phase(pomo, at(13)),
            Err(EngineError::AlreadyRunning)
        );
        assert_eq!(
            timers.start_phase(sw, at(13)),
            Err(EngineError::NotPomodoro(sw))
        );
        let missing = TimerId(99);
        assert_eq!(
            timers.pause(missing, at(1)),
            Err(EngineError::TimerNotFound(missing))
        );
    }

    #[test]
    fn second_round_sessions_carry_round_two() {
        let mut timers = Timers::new();
        let config = PomodoroConfig::new(secs(10), secs(10), secs(10), 1, true).unwrap();
        timers.add(None, Timer::Pomodoro(Pomodoro::start(config, at(0))));
        let events = timers.tick(at(35));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, TimersEvent::SessionCompleted { round: Some(2), .. }))
        );
    }
}
