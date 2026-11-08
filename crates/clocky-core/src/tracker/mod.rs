//! Tracker: the facade that ties the timer engine to the store.
//!
//! Every command runs on a copy of the timers, writes the result (sessions plus active-timer
//! rows) in one transaction and only then replaces the timers in memory. If the database fails,
//! memory stays as it was: the database is the source of truth (design decisions 7 and 8).

mod error;

pub use error::TrackerError;

use std::time::Duration;

use crate::domain::TimerPresetId;
use crate::domain::{PomodoroPresetId, PresetRef, SessionId, SubtaskId, TaskId, TimerId};
use crate::engine::EngineError;
use crate::engine::countdown::Countdown;
use crate::engine::pomodoro::{Pomodoro, PomodoroConfig};
use crate::engine::stopwatch::Stopwatch;
use crate::engine::timers::{ActiveTimer, Timer, Timers, TimersEvent};
use crate::store::Store;
use crate::store::writes::{SessionToRecord, TimerWrites};
use crate::time::Timestamp;

/// Event reported by the tracker.
///
/// Every engine event is forwarded as [`TrackerEvent::Timer`], in order. Each
/// [`TimersEvent::SessionCompleted`] is immediately followed by [`TrackerEvent::SessionRecorded`]
/// with the identifier the store gave that session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackerEvent {
    /// An engine event (session completed, pomodoro phase ended, countdown finished).
    Timer(TimersEvent),
    /// The session of the preceding [`TimersEvent::SessionCompleted`] was recorded.
    SessionRecorded {
        /// Timer that produced the session.
        timer: TimerId,
        /// Identifier of the recorded session.
        session: SessionId,
    },
}

/// The store and the active timers, kept in sync.
#[derive(Debug)]
pub struct Tracker {
    store: Store,
    timers: Timers,
    /// The store was lent out mutably: a task or preset may have been deleted, so the links of
    /// the timers must be read back before the next write.
    links_stale: bool,
}

impl Tracker {
    /// Restores the saved timers and applies the reopen rules with one tick at `now`: a
    /// countdown that came due is recorded at its due time, an overdue pomodoro phase closes at
    /// its scheduled time and waits, a stopwatch keeps counting (see [`Tracker::forgotten`]).
    /// Returns the events of that tick, so the app can say what happened while it was closed.
    ///
    /// # Errors
    /// [`TrackerError::Store`] if a saved timer is corrupt or the database fails.
    pub fn open(store: Store, now: Timestamp) -> Result<(Self, Vec<TrackerEvent>), TrackerError> {
        let (next_id, saved) = store.load_active()?;
        let timers = Timers::restore(next_id, saved)?;
        let mut tracker = Self {
            store,
            timers,
            links_stale: false,
        };
        let events = tracker.tick(now)?;
        Ok((tracker, events))
    }

    /// The active timers.
    #[must_use]
    pub fn timers(&self) -> &Timers {
        &self.timers
    }

    /// The store, for queries.
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The store, for CRUD (projects, tasks, presets, settings, sessions).
    ///
    /// The tracker reads the task and preset of its timers back from the database before its
    /// next command, so deleting a task or preset here unlinks the timers too. Do not save or
    /// delete active timers through it: use the tracker commands.
    pub fn store_mut(&mut self) -> &mut Store {
        self.links_stale = true;
        &mut self.store
    }

    /// Starts a stopwatch.
    ///
    /// # Errors
    /// [`TrackerError::Store`] if the task does not exist or the database fails.
    pub fn start_stopwatch(
        &mut self,
        task: Option<TaskId>,
        now: Timestamp,
    ) -> Result<TimerId, TrackerError> {
        self.start(task, None, Timer::Stopwatch(Stopwatch::start(now)))
    }

    /// Starts a countdown of `target`, optionally from a saved timer.
    ///
    /// # Errors
    /// [`TrackerError::Store`] if the task does not exist or the database fails.
    pub fn start_countdown(
        &mut self,
        task: Option<TaskId>,
        target: Duration,
        preset: Option<TimerPresetId>,
        now: Timestamp,
    ) -> Result<TimerId, TrackerError> {
        let timer = Timer::Countdown(Countdown::start(now, target));
        self.start(task, preset.map(PresetRef::Timer), timer)
    }

    /// Starts a pomodoro, optionally from a saved pomodoro (its configuration is copied).
    ///
    /// # Errors
    /// [`TrackerError::Store`] if the task does not exist or the database fails.
    pub fn start_pomodoro(
        &mut self,
        task: Option<TaskId>,
        config: PomodoroConfig,
        preset: Option<PomodoroPresetId>,
        now: Timestamp,
    ) -> Result<TimerId, TrackerError> {
        let timer = Timer::Pomodoro(Pomodoro::start(config, now));
        self.start(task, preset.map(PresetRef::Pomodoro), timer)
    }

    /// Pauses a timer.
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if the timer does not exist or is not running;
    /// [`TrackerError::Store`] if the database fails.
    pub fn pause(&mut self, id: TimerId, now: Timestamp) -> Result<(), TrackerError> {
        self.apply(|timers| timers.pause(id, now).map(no_events))
            .map(drop)
    }

    /// Resumes a paused timer.
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if the timer does not exist, is running or is a waiting
    /// pomodoro phase; [`TrackerError::Store`] if the database fails.
    pub fn resume(&mut self, id: TimerId, now: Timestamp) -> Result<(), TrackerError> {
        self.apply(|timers| timers.resume(id, now).map(no_events))
            .map(drop)
    }

    /// Starts the waiting phase of a pomodoro.
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if the timer does not exist, is not a pomodoro or its phase has
    /// started; [`TrackerError::Store`] if the database fails.
    pub fn start_phase(&mut self, id: TimerId, now: Timestamp) -> Result<(), TrackerError> {
        self.apply(|timers| timers.start_phase(id, now).map(no_events))
            .map(drop)
    }

    /// Stops a timer and records its session ("Record all" in the forgotten dialog).
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if the timer does not exist; [`TrackerError::Store`] if the
    /// database fails.
    pub fn stop(&mut self, id: TimerId, now: Timestamp) -> Result<Vec<TrackerEvent>, TrackerError> {
        self.apply(|timers| Ok(((), timers.stop(id, now)?.into_iter().collect())))
            .map(|((), events)| events)
    }

    /// Skips the current phase of a pomodoro (an interrupted focus is recorded).
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if the timer does not exist or is not a pomodoro;
    /// [`TrackerError::Store`] if the database fails.
    pub fn skip(&mut self, id: TimerId, now: Timestamp) -> Result<Vec<TrackerEvent>, TrackerError> {
        self.apply(|timers| Ok(((), timers.skip(id, now)?)))
            .map(|((), events)| events)
    }

    /// Changes the task of a timer; it applies to the whole session.
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if the timer does not exist; [`TrackerError::Store`] if the task
    /// does not exist or the database fails.
    pub fn set_task(&mut self, id: TimerId, task: Option<TaskId>) -> Result<(), TrackerError> {
        self.apply(|timers| timers.set_task(id, task).map(no_events))
            .map(drop)
    }

    /// Removes a timer without recording anything ("Discard" in the forgotten dialog).
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if the timer does not exist; [`TrackerError::Store`] if the
    /// database fails.
    pub fn discard(&mut self, id: TimerId) -> Result<(), TrackerError> {
        self.apply(|timers| timers.discard(id).map(drop).map(no_events))
            .map(drop)
    }

    /// Advances every timer to `now` and records what came due. Only timers whose state changed
    /// are written. A gap of more than a minute since the last tick (suspension) gets the same
    /// treatment as reopening: overdue phases close at their scheduled time.
    ///
    /// # Errors
    /// [`TrackerError::Store`] if the database fails (memory is unchanged, so the next tick
    /// retries).
    pub fn tick(&mut self, now: Timestamp) -> Result<Vec<TrackerEvent>, TrackerError> {
        self.apply(|timers| Ok(((), timers.tick(now))))
            .map(|((), events)| events)
    }

    /// Running stopwatches that ran longer than the store's forgotten limit (the "forgotten?"
    /// dialog). Paused stopwatches are not listed.
    ///
    /// # Errors
    /// [`TrackerError::Store`] if the stored limit is invalid.
    pub fn forgotten(&self, now: Timestamp) -> Result<Vec<TimerId>, TrackerError> {
        let limit = self.store.forgotten_limit()?;
        Ok(self
            .timers
            .iter()
            .filter(|active| match &active.timer {
                Timer::Stopwatch(sw) => sw.is_running() && sw.exceeds(now, limit),
                _ => false,
            })
            .map(ActiveTimer::id)
            .collect())
    }

    /// Checks a subtask at `at`, clicked in the checklist of timer `run` (`None` = outside a
    /// timer). The event is linked to the session of `run` that contains it.
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if `run` is not an active timer; [`TrackerError::Store`] if the
    /// subtask does not exist or the database fails.
    pub fn complete_subtask(
        &mut self,
        subtask: SubtaskId,
        at: Timestamp,
        run: Option<TimerId>,
    ) -> Result<(), TrackerError> {
        self.check_run(run)?;
        Ok(self.store.complete_subtask(subtask, at, run)?)
    }

    /// Unchecks a subtask at `at` (same rules as [`Tracker::complete_subtask`]).
    ///
    /// # Errors
    /// [`TrackerError::Engine`] if `run` is not an active timer; [`TrackerError::Store`] if the
    /// subtask does not exist or the database fails.
    pub fn reopen_subtask(
        &mut self,
        subtask: SubtaskId,
        at: Timestamp,
        run: Option<TimerId>,
    ) -> Result<(), TrackerError> {
        self.check_run(run)?;
        Ok(self.store.reopen_subtask(subtask, at, run)?)
    }

    fn check_run(&self, run: Option<TimerId>) -> Result<(), EngineError> {
        match run {
            Some(id) if self.timers.get(id).is_none() => Err(EngineError::TimerNotFound(id)),
            _ => Ok(()),
        }
    }

    fn start(
        &mut self,
        task: Option<TaskId>,
        preset: Option<PresetRef>,
        timer: Timer,
    ) -> Result<TimerId, TrackerError> {
        let (id, _) = self.apply(|timers| {
            let id = timers.add(task, timer);
            if let Some(active) = timers.get_mut(id) {
                active.preset = preset;
            }
            Ok((id, Vec::new()))
        })?;
        Ok(id)
    }

    /// Runs `command` on a copy of the timers, writes the sessions and the changed timers in one
    /// transaction and only then replaces the timers in memory (decision 8).
    fn apply<T>(
        &mut self,
        command: impl FnOnce(&mut Timers) -> Result<(T, Vec<TimersEvent>), EngineError>,
    ) -> Result<(T, Vec<TrackerEvent>), TrackerError> {
        self.sync_links()?;
        let mut next = self.timers.clone();
        let (value, events) = command(&mut next)?;
        let writes = changes(&self.timers, &next, &events);
        let ids = if writes.is_empty() {
            Vec::new()
        } else {
            self.store.write_timers(&writes)?
        };
        self.timers = next;
        Ok((value, with_session_ids(events, ids)))
    }

    /// Reads the task and preset of every timer back from the database after the store was
    /// lent out mutably (a deleted task or preset is cleared there by the schema).
    fn sync_links(&mut self) -> Result<(), TrackerError> {
        if !self.links_stale {
            return Ok(());
        }
        let (_, saved) = self.store.load_active()?;
        for saved in saved {
            if let Some(active) = self.timers.get_mut(saved.id()) {
                active.task = saved.task;
                active.preset = saved.preset;
            }
        }
        self.links_stale = false;
        Ok(())
    }
}

/// A command result without events.
fn no_events<T>(value: T) -> (T, Vec<TimersEvent>) {
    (value, Vec::new())
}

/// What to write to go from `old` to `new`: the completed sessions, the timers that are new or
/// changed, and the ones that left the set.
fn changes<'a>(old: &Timers, new: &'a Timers, events: &'a [TimersEvent]) -> TimerWrites<'a> {
    let sessions = events
        .iter()
        .filter_map(|event| match event {
            TimersEvent::SessionCompleted {
                timer,
                task,
                session,
                round,
            } => Some(SessionToRecord {
                task: *task,
                run: *timer,
                round: *round,
                session,
            }),
            _ => None,
        })
        .collect();
    TimerWrites {
        sessions,
        save: new
            .iter()
            .filter(|active| old.get(active.id()) != Some(*active))
            .collect(),
        delete: old
            .iter()
            .map(ActiveTimer::id)
            .filter(|id| new.get(*id).is_none())
            .collect(),
    }
}

/// Forwards the engine events, adding a [`TrackerEvent::SessionRecorded`] after each completed
/// session (`ids` are the recorded sessions, in the same order).
fn with_session_ids(events: Vec<TimersEvent>, ids: Vec<SessionId>) -> Vec<TrackerEvent> {
    let mut ids = ids.into_iter();
    let mut out = Vec::with_capacity(events.len() * 2);
    for event in events {
        let recorded = match &event {
            TimersEvent::SessionCompleted { timer, .. } => {
                ids.next().map(|session| TrackerEvent::SessionRecorded {
                    timer: *timer,
                    session,
                })
            }
            _ => None,
        };
        out.push(TrackerEvent::Timer(event));
        out.extend(recorded);
    }
    out
}

#[cfg(test)]
mod tests;
