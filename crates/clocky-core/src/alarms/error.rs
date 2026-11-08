//! Alarm errors.

/// Errors from alarm values and operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AlarmError {
    /// The weekday bitmask has bits above Sunday set.
    #[error("invalid weekday bitmask {0:#04x}: only the lowest 7 bits may be set")]
    InvalidWeekdays(u8),
}
