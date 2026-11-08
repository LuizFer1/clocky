//! Alarm errors.

/// Errors from alarm values and operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AlarmError {
    /// The weekday bitmask has bits above Sunday set.
    #[error("invalid weekday bitmask {0:#04x}: only the lowest 7 bits may be set")]
    InvalidWeekdays(u8),
    /// The text is not a valid `HH:MM` time (00:00 to 23:59).
    #[error("invalid alarm time {0:?}: expected HH:MM between 00:00 and 23:59")]
    InvalidTime(String),
    /// A snooze must last at least one minute.
    #[error("invalid snooze: it must last at least 1 minute")]
    InvalidSnooze,
}
