//! Domain identifiers.

use std::fmt;

/// Identifier of a project (assigned by storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(pub i64);

/// Identifier of a task (assigned by storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub i64);

/// Identifier of a subtask (assigned by storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubtaskId(pub i64);

/// Identifier of a countdown timer preset (assigned by storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerPresetId(pub i64);

/// Identifier of a pomodoro preset (assigned by storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PomodoroPresetId(pub i64);

/// Identifier of a recorded session (assigned by storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub i64);

/// Identifier of an active timer.
///
/// Persistent and never reused: it also identifies the timer's run, which groups the sessions of
/// a pomodoro and links subtask events to the timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(pub u64);

impl fmt::Display for TimerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}
