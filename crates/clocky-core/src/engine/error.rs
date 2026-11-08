//! Timer engine errors.

use crate::domain::TimerId;

/// Errors from timer operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    /// The timer is not running.
    #[error("the timer is not running")]
    NotRunning,
    /// The timer is already running.
    #[error("the timer is already running")]
    AlreadyRunning,
    /// Invalid pomodoro configuration.
    #[error(
        "invalid pomodoro configuration: each phase needs at least 1 second and a round needs at least 1 focus"
    )]
    InvalidConfig,
    /// No active timer has this identifier.
    #[error("timer {0} not found")]
    TimerNotFound(TimerId),
    /// The timer is not a pomodoro.
    #[error("timer {0} is not a pomodoro")]
    NotPomodoro(TimerId),
    /// The parts of a run being restored are inconsistent.
    #[error("invalid run: segments must be ordered, non-overlapping and not inverted")]
    InvalidRun,
    /// The state of a pomodoro being restored is inconsistent with its configuration.
    #[error("invalid pomodoro state: round, focus index and phase do not match the configuration")]
    InvalidPomodoroState,
    /// A restored timer repeats an identifier or is not below the next identifier.
    #[error("timer {0} is duplicated or not below the next timer id")]
    InvalidTimerId(TimerId),
}
