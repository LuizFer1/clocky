//! Pomodoro: focus and break phases organized into rounds.

use std::time::Duration;

use super::error::EngineError;
use super::run::Run;
use super::session::{CompletedSession, TimerKind};
use crate::time::Timestamp;

/// Phase of the pomodoro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Focus (produces a session).
    Focus,
    /// Short break, between focus phases.
    ShortBreak,
    /// Long break, at the end of each round.
    LongBreak,
}

/// Maximum delay, after the scheduled end of a phase, for it to still be treated as "real time".
///
/// Beyond that (app closed or PC suspended), the phase closes at its scheduled time and the
/// pomodoro stops and waits, without starting the next phase on its own (spec decision 1).
pub const CATCH_UP_GRACE: Duration = Duration::from_secs(60);

/// Event produced by the pomodoro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PomodoroEvent {
    /// A focus phase ended (completed or interrupted) and became a session.
    FocusSession {
        /// Session to record.
        session: CompletedSession,
        /// Round the focus belongs to (groups sessions in the report).
        round: u32,
    },
    /// A phase ended.
    PhaseEnded {
        /// Phase that ended.
        ended: Phase,
        /// Next phase.
        next: Phase,
        /// Did the next phase start on its own?
        auto_started: bool,
    },
}

/// How a phase is being closed.
#[derive(Debug, Clone, Copy)]
enum Close {
    /// By the clock, at the scheduled time. `late` = arrived after the grace period.
    Scheduled { at: Timestamp, late: bool },
    /// By user action at `now` (skip).
    User(Timestamp),
}

/// Configuration of a pomodoro, copied from the preset when it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PomodoroConfig {
    focus: Duration,
    short_break: Duration,
    long_break: Duration,
    focus_per_round: u32,
    auto_start: bool,
}

impl PomodoroConfig {
    /// Creates a configuration.
    ///
    /// # Errors
    /// [`EngineError::InvalidConfig`] if any phase lasts less than 1 second
    /// or if `focus_per_round` is zero.
    pub fn new(
        focus: Duration,
        short_break: Duration,
        long_break: Duration,
        focus_per_round: u32,
        auto_start: bool,
    ) -> Result<Self, EngineError> {
        let min = Duration::from_secs(1);
        if focus < min || short_break < min || long_break < min || focus_per_round == 0 {
            return Err(EngineError::InvalidConfig);
        }
        Ok(Self {
            focus,
            short_break,
            long_break,
            focus_per_round,
            auto_start,
        })
    }

    /// Duration of a phase.
    #[must_use]
    pub fn duration(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Focus => self.focus,
            Phase::ShortBreak => self.short_break,
            Phase::LongBreak => self.long_break,
        }
    }

    /// Focus phases per round (before the long break).
    #[must_use]
    pub fn focus_per_round(&self) -> u32 {
        self.focus_per_round
    }

    /// Do the following phases start on their own?
    #[must_use]
    pub fn auto_start(&self) -> bool {
        self.auto_start
    }
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {
            focus: Duration::from_mins(25),
            short_break: Duration::from_mins(5),
            long_break: Duration::from_mins(15),
            focus_per_round: 4,
            auto_start: false,
        }
    }
}

/// A pomodoro in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pomodoro {
    config: PomodoroConfig,
    phase: Phase,
    focus_index: u32,
    round: u32,
    /// `None` = the current phase has not started yet (waiting for the user).
    run: Option<Run>,
}

impl Pomodoro {
    /// Creates the pomodoro already running its first focus phase from `now`.
    #[must_use]
    pub fn start(config: PomodoroConfig, now: Timestamp) -> Self {
        Self {
            config,
            phase: Phase::Focus,
            focus_index: 1,
            round: 1,
            run: Some(Run::start(now)),
        }
    }

    /// Restores a pomodoro from its parts. `run` is `None` while the phase waits for the user.
    ///
    /// # Errors
    /// [`EngineError::InvalidPomodoroState`] if `round` is zero, `focus_index` is outside
    /// `1..=focus_per_round`, or the phase cannot happen at that focus index (the long break
    /// only after the last focus of the round, the short break only before it).
    pub fn from_parts(
        config: PomodoroConfig,
        phase: Phase,
        focus_index: u32,
        round: u32,
        run: Option<Run>,
    ) -> Result<Self, EngineError> {
        let last = config.focus_per_round;
        let phase_fits = match phase {
            Phase::Focus => true,
            Phase::ShortBreak => focus_index < last,
            Phase::LongBreak => focus_index == last,
        };
        if round == 0 || focus_index == 0 || focus_index > last || !phase_fits {
            return Err(EngineError::InvalidPomodoroState);
        }
        Ok(Self {
            config,
            phase,
            focus_index,
            round,
            run,
        })
    }

    /// Configuration copied when it started.
    #[must_use]
    pub fn config(&self) -> &PomodoroConfig {
        &self.config
    }

    /// Run of the current phase; `None` while it waits for the user.
    #[must_use]
    pub fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Position of the focus phase in the round (1 up to the focus phases per round), the UI's
    /// "focus 2/4".
    #[must_use]
    pub fn focus_index(&self) -> u32 {
        self.focus_index
    }

    /// Round number (starts at 1 and increases after each long break).
    #[must_use]
    pub fn round(&self) -> u32 {
        self.round
    }

    /// Is the current phase waiting for the user to start it?
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        self.run.is_none()
    }

    /// Is the current phase running (started and not paused)?
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.run.as_ref().is_some_and(Run::is_running)
    }

    /// Time remaining in the current phase at `now`.
    #[must_use]
    pub fn remaining(&self, now: Timestamp) -> Duration {
        let total = self.config.duration(self.phase);
        match &self.run {
            Some(run) => total.saturating_sub(run.elapsed(now)),
            None => total,
        }
    }

    /// Starts the current phase, when it is waiting.
    ///
    /// # Errors
    /// [`EngineError::AlreadyRunning`] if the phase has already started.
    pub fn start_phase(&mut self, now: Timestamp) -> Result<(), EngineError> {
        if self.run.is_some() {
            return Err(EngineError::AlreadyRunning);
        }
        self.run = Some(Run::start(now));
        Ok(())
    }

    /// Pauses the current phase.
    ///
    /// # Errors
    /// [`EngineError::NotRunning`] if the phase is waiting or already paused.
    pub fn pause(&mut self, now: Timestamp) -> Result<(), EngineError> {
        self.run.as_mut().ok_or(EngineError::NotRunning)?.pause(now)
    }

    /// Resumes the current phase.
    ///
    /// # Errors
    /// [`EngineError::NotRunning`] if the phase is waiting (use `start_phase`);
    /// [`EngineError::AlreadyRunning`] if already running.
    pub fn resume(&mut self, now: Timestamp) -> Result<(), EngineError> {
        self.run
            .as_mut()
            .ok_or(EngineError::NotRunning)?
            .resume(now)
    }

    /// Advances time up to `now`: closes the phases that came due and returns the events in
    /// order.
    #[must_use = "events must be processed (sessions to record)"]
    pub fn tick(&mut self, now: Timestamp) -> Vec<PomodoroEvent> {
        let mut events = Vec::new();
        while let Some(end) = self.phase_end() {
            if end > now {
                break;
            }
            let late = now.since(end) > CATCH_UP_GRACE;
            events.extend(self.end_phase(Close::Scheduled { at: end, late }));
        }
        events
    }

    /// Skips the current phase: ends it now (an interrupted focus becomes a partial session)
    /// and moves on to the next one, which follows the normal start rules.
    ///
    /// If the phase is already due, acts like [`Pomodoro::tick`]: closes at the scheduled time.
    #[must_use = "events must be processed (sessions to record)"]
    pub fn skip(&mut self, now: Timestamp) -> Vec<PomodoroEvent> {
        if self.phase_end().is_some_and(|end| end <= now) {
            return self.tick(now);
        }
        self.end_phase(Close::User(now))
    }

    /// Ends the pomodoro. If in focus, returns the partial session
    /// (a focus phase that is already due is capped at its scheduled time).
    #[must_use]
    pub fn stop(self, now: Timestamp) -> Option<CompletedSession> {
        if self.phase != Phase::Focus {
            return None;
        }
        let due = self.phase_end().filter(|due| *due <= now);
        let run = self.run?;
        let segments = match due {
            Some(due) => run.finish(due),
            None => run.stop(now),
        };
        CompletedSession::new(TimerKind::Pomodoro, segments)
    }

    fn phase_end(&self) -> Option<Timestamp> {
        self.run
            .as_ref()?
            .reaches_at(self.config.duration(self.phase))
    }

    /// Closes the current phase. A late scheduled close (`late`), or one with the phase paused,
    /// never starts the next phase on its own.
    fn end_phase(&mut self, close: Close) -> Vec<PomodoroEvent> {
        let mut events = Vec::new();
        let ended = self.phase;
        let round = self.round;
        let was_running = self.is_running();
        if let Some(run) = self.run.take()
            && ended == Phase::Focus
        {
            let segments = match close {
                Close::Scheduled { at, .. } => run.finish(at),
                Close::User(now) => run.stop(now),
            };
            if let Some(session) = CompletedSession::new(TimerKind::Pomodoro, segments) {
                events.push(PomodoroEvent::FocusSession { session, round });
            }
        }
        self.advance();
        let (next_start, may_auto_start) = match close {
            Close::Scheduled { at, late } => (at, !late && was_running),
            Close::User(now) => (now, true),
        };
        let auto_started = may_auto_start && (ended == Phase::LongBreak || self.config.auto_start);
        if auto_started {
            self.run = Some(Run::start(next_start));
        }
        events.push(PomodoroEvent::PhaseEnded {
            ended,
            next: self.phase,
            auto_started,
        });
        events
    }

    fn advance(&mut self) {
        self.phase = match self.phase {
            Phase::Focus if self.focus_index >= self.config.focus_per_round => Phase::LongBreak,
            Phase::Focus => Phase::ShortBreak,
            Phase::ShortBreak => {
                self.focus_index += 1;
                Phase::Focus
            }
            Phase::LongBreak => {
                self.round += 1;
                self.focus_index = 1;
                Phase::Focus
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::run::Segment;
    use crate::engine::session::{CompletedSession, TimerKind};
    use crate::test_util::{at, mins, secs};

    /// 25/5/15 with 2 focus phases per round (short rounds keep the tests readable).
    fn cfg(auto_start: bool) -> PomodoroConfig {
        PomodoroConfig::new(mins(25), mins(5), mins(15), 2, auto_start).unwrap()
    }

    #[test]
    fn default_config_is_classic() {
        let c = PomodoroConfig::default();
        assert_eq!(c.duration(Phase::Focus), mins(25));
        assert_eq!(c.duration(Phase::ShortBreak), mins(5));
        assert_eq!(c.duration(Phase::LongBreak), mins(15));
        assert_eq!(c.focus_per_round(), 4);
        assert!(!c.auto_start());
    }

    #[test]
    fn config_rejects_tiny_phases_and_empty_rounds() {
        let tiny = Duration::from_millis(500);
        assert_eq!(
            PomodoroConfig::new(tiny, mins(5), mins(15), 4, false),
            Err(EngineError::InvalidConfig)
        );
        assert_eq!(
            PomodoroConfig::new(mins(25), mins(5), mins(15), 0, false),
            Err(EngineError::InvalidConfig)
        );
    }

    #[test]
    fn starts_in_first_focus_of_first_round() {
        let p = Pomodoro::start(cfg(false), at(0));
        assert_eq!(
            (p.phase(), p.focus_index(), p.round()),
            (Phase::Focus, 1, 1)
        );
        assert!(p.is_running());
        assert_eq!(p.remaining(at(60)), mins(24));
    }

    #[test]
    fn pause_and_resume_freeze_the_phase() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        p.pause(at(60)).unwrap();
        assert!(!p.is_running());
        assert_eq!(p.remaining(at(600)), mins(24));
        p.resume(at(600)).unwrap();
        assert_eq!(p.remaining(at(660)), mins(23));
    }

    #[test]
    fn start_phase_while_running_is_an_error() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        assert_eq!(p.start_phase(at(1)), Err(EngineError::AlreadyRunning));
        assert!(!p.is_waiting());
    }

    fn focus_session(start: i64, end: i64) -> CompletedSession {
        CompletedSession::new(
            TimerKind::Pomodoro,
            vec![Segment {
                start: at(start),
                end: at(end),
            }],
        )
        .unwrap()
    }

    #[test]
    fn tick_before_phase_end_does_nothing() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        assert_eq!(p.tick(at(60)), vec![]);
    }

    #[test]
    fn focus_end_emits_session_and_waits_for_break() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        let events = p.tick(at(25 * 60));
        assert_eq!(
            events,
            vec![
                PomodoroEvent::FocusSession {
                    session: focus_session(0, 25 * 60),
                    round: 1
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::Focus,
                    next: Phase::ShortBreak,
                    auto_started: false,
                },
            ]
        );
        assert_eq!(p.phase(), Phase::ShortBreak);
        assert!(p.is_waiting());
        assert_eq!(p.remaining(at(9999)), mins(5));
    }

    #[test]
    fn auto_start_begins_next_phase_at_scheduled_end() {
        let mut p = Pomodoro::start(cfg(true), at(0));
        // tick 30s late (within the grace period): the break started at 25:00, not 25:30
        let _events = p.tick(at(25 * 60 + 30));
        assert_eq!(p.phase(), Phase::ShortBreak);
        assert!(p.is_running());
        assert_eq!(p.remaining(at(25 * 60 + 30)), secs(270));
        let _events = p.tick(at(30 * 60));
        assert_eq!((p.phase(), p.focus_index()), (Phase::Focus, 2));
        assert!(p.is_running());
    }

    #[test]
    fn last_focus_of_round_leads_to_long_break_then_new_round() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        let _events = p.tick(at(1500)); // focus 1 -> short break (waiting)
        p.start_phase(at(1500)).unwrap();
        let _events = p.tick(at(1800)); // short break -> focus 2 (waiting)
        assert_eq!((p.phase(), p.focus_index()), (Phase::Focus, 2));
        p.start_phase(at(1800)).unwrap();
        let _events = p.tick(at(3300)); // focus 2 (last of the round) -> long break
        assert_eq!(p.phase(), Phase::LongBreak);
        p.start_phase(at(3300)).unwrap();
        let events = p.tick(at(4200)); // long break ends
        assert_eq!(
            events,
            vec![PomodoroEvent::PhaseEnded {
                ended: Phase::LongBreak,
                next: Phase::Focus,
                auto_started: true,
            }]
        );
        assert_eq!((p.round(), p.focus_index()), (2, 1));
        assert!(p.is_running());
    }

    #[test]
    fn late_tick_closes_phase_at_scheduled_end_and_waits() {
        let mut p = Pomodoro::start(cfg(true), at(0));
        let events = p.tick(at(3 * 3600)); // app closed for 3h
        assert_eq!(
            events,
            vec![
                PomodoroEvent::FocusSession {
                    session: focus_session(0, 1500),
                    round: 1
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::Focus,
                    next: Phase::ShortBreak,
                    auto_started: false,
                },
            ]
        );
        assert!(p.is_waiting());
    }

    #[test]
    fn late_long_break_does_not_start_a_new_round() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        let _events = p.skip(at(10)); // focus 1 -> short break
        let _events = p.skip(at(20)); // short break -> focus 2
        let _events = p.skip(at(30)); // focus 2 (waiting) -> long break
        p.start_phase(at(30)).unwrap();
        let events = p.tick(at(30 + 900 + 120));
        assert_eq!(
            events,
            vec![PomodoroEvent::PhaseEnded {
                ended: Phase::LongBreak,
                next: Phase::Focus,
                auto_started: false,
            }]
        );
        assert_eq!(p.round(), 2);
        assert!(p.is_waiting());
    }

    #[test]
    fn skip_focus_records_partial_session() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        assert_eq!(
            p.skip(at(600)),
            vec![
                PomodoroEvent::FocusSession {
                    session: focus_session(0, 600),
                    round: 1
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::Focus,
                    next: Phase::ShortBreak,
                    auto_started: false,
                },
            ]
        );
    }

    #[test]
    fn skip_while_waiting_advances_without_session() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        let _events = p.tick(at(1500)); // waiting for the short break
        assert_eq!(
            p.skip(at(1600)),
            vec![PomodoroEvent::PhaseEnded {
                ended: Phase::ShortBreak,
                next: Phase::Focus,
                auto_started: false,
            }]
        );
        assert_eq!(p.focus_index(), 2);
    }

    #[test]
    fn stop_during_focus_returns_partial_session() {
        let p = Pomodoro::start(cfg(false), at(0));
        assert_eq!(p.stop(at(600)), Some(focus_session(0, 600)));
    }

    #[test]
    fn stop_during_break_records_nothing() {
        let mut p = Pomodoro::start(cfg(true), at(0));
        let _events = p.tick(at(1500)); // short break running
        assert_eq!(p.stop(at(1600)), None);
    }

    #[test]
    fn paused_phase_never_ends() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        p.pause(at(600)).unwrap();
        assert_eq!(p.tick(at(7200)), vec![]);
        assert_eq!(p.remaining(at(7200)), mins(15));
    }

    #[test]
    fn stop_after_scheduled_end_caps_the_session() {
        let p = Pomodoro::start(cfg(false), at(0));
        assert_eq!(p.stop(at(3 * 3600)), Some(focus_session(0, 1500)));
    }

    #[test]
    fn stop_when_paused_after_due_caps_the_session() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        p.pause(at(1600)).unwrap();
        assert_eq!(p.stop(at(9000)), Some(focus_session(0, 1500)));
    }

    #[test]
    fn skip_after_scheduled_end_behaves_like_tick() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        assert_eq!(
            p.skip(at(3 * 3600)),
            vec![
                PomodoroEvent::FocusSession {
                    session: focus_session(0, 1500),
                    round: 1
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::Focus,
                    next: Phase::ShortBreak,
                    auto_started: false,
                },
            ]
        );
    }

    #[test]
    fn skip_with_clock_moved_back_keeps_focus_time() {
        let mut p = Pomodoro::start(cfg(false), at(1000));
        p.pause(at(1600)).unwrap();
        let events = p.skip(at(500));
        assert_eq!(
            events[0],
            PomodoroEvent::FocusSession {
                session: focus_session(1000, 1600),
                round: 1
            }
        );
    }

    #[test]
    fn pause_right_after_due_blocks_auto_start() {
        let mut p = Pomodoro::start(cfg(true), at(0));
        p.pause(at(1510)).unwrap();
        assert_eq!(
            p.tick(at(1511)),
            vec![
                PomodoroEvent::FocusSession {
                    session: focus_session(0, 1500),
                    round: 1
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::Focus,
                    next: Phase::ShortBreak,
                    auto_started: false,
                },
            ]
        );
        assert!(p.is_waiting());
    }

    #[test]
    fn one_tick_can_cross_several_auto_started_phases() {
        let config = PomodoroConfig::new(secs(10), secs(10), secs(10), 2, true).unwrap();
        let mut p = Pomodoro::start(config, at(0));
        assert_eq!(
            p.tick(at(35)),
            vec![
                PomodoroEvent::FocusSession {
                    session: focus_session(0, 10),
                    round: 1
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::Focus,
                    next: Phase::ShortBreak,
                    auto_started: true,
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::ShortBreak,
                    next: Phase::Focus,
                    auto_started: true,
                },
                PomodoroEvent::FocusSession {
                    session: focus_session(20, 30),
                    round: 1
                },
                PomodoroEvent::PhaseEnded {
                    ended: Phase::Focus,
                    next: Phase::LongBreak,
                    auto_started: true,
                },
            ]
        );
        assert_eq!(p.phase(), Phase::LongBreak);
        assert!(p.is_running());
        assert_eq!(p.remaining(at(35)), secs(5));
    }

    #[test]
    fn grace_boundary_is_sixty_seconds() {
        let mut on_time = Pomodoro::start(cfg(true), at(0));
        let _events = on_time.tick(at(1500 + 60));
        assert_eq!(on_time.phase(), Phase::ShortBreak);
        assert!(on_time.is_running());
        let mut late = Pomodoro::start(cfg(true), at(0));
        let _events = late.tick(at(1500 + 61));
        assert_eq!(late.phase(), Phase::ShortBreak);
        assert!(late.is_waiting());
    }

    #[test]
    fn from_parts_restores_every_field() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        let _events = p.tick(at(1500)); // short break waiting
        let restored = Pomodoro::from_parts(
            p.config().clone(),
            p.phase(),
            p.focus_index(),
            p.round(),
            p.run().cloned(),
        )
        .unwrap();
        assert_eq!(restored, p);
        assert!(restored.is_waiting());
        let running = Pomodoro::start(cfg(true), at(0));
        let restored = Pomodoro::from_parts(cfg(true), Phase::Focus, 1, 1, running.run().cloned());
        assert_eq!(restored.unwrap(), running);
    }

    #[test]
    fn from_parts_rejects_impossible_states() {
        let invalid = Err(EngineError::InvalidPomodoroState);
        assert_eq!(
            Pomodoro::from_parts(cfg(false), Phase::Focus, 1, 0, None),
            invalid
        );
        assert_eq!(
            Pomodoro::from_parts(cfg(false), Phase::Focus, 0, 1, None),
            invalid
        );
        assert_eq!(
            Pomodoro::from_parts(cfg(false), Phase::Focus, 3, 1, None),
            invalid
        );
        assert_eq!(
            Pomodoro::from_parts(cfg(false), Phase::ShortBreak, 2, 1, None),
            invalid
        );
        assert_eq!(
            Pomodoro::from_parts(cfg(false), Phase::LongBreak, 1, 1, None),
            invalid
        );
        assert!(Pomodoro::from_parts(cfg(false), Phase::LongBreak, 2, 3, None).is_ok());
        assert!(Pomodoro::from_parts(cfg(false), Phase::ShortBreak, 1, 1, None).is_ok());
    }

    #[test]
    fn stop_while_waiting_records_nothing() {
        let mut p = Pomodoro::start(cfg(false), at(0));
        let _events = p.tick(at(1500)); // short break waiting
        let _events = p.skip(at(1500)); // focus 2 waiting
        assert_eq!((p.phase(), p.focus_index()), (Phase::Focus, 2));
        assert!(p.is_waiting());
        assert_eq!(p.stop(at(2000)), None);
    }
}
