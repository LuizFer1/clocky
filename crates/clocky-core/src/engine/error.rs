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
}
