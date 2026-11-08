//! Checked conversions between Rust types and SQLite integers (always `i64`, in milliseconds).

use std::time::Duration;

use super::StoreError;
use crate::domain::TimerId;

/// An `INTEGER` read from the database as `u32` (counts, versions, rounds).
pub(super) fn u32_from_sql(value: i64, what: &'static str) -> Result<u32, StoreError> {
    u32::try_from(value).map_err(|_| StoreError::OutOfRange(what))
}

/// A [`TimerId`] (`u64`) as an `INTEGER`.
pub(super) fn timer_id_to_sql(id: TimerId) -> Result<i64, StoreError> {
    i64::try_from(id.0).map_err(|_| StoreError::OutOfRange("timer id"))
}

/// An `INTEGER` read from the database as a [`TimerId`].
pub(super) fn timer_id_from_sql(value: i64) -> Result<TimerId, StoreError> {
    u64::try_from(value)
        .map(TimerId)
        .map_err(|_| StoreError::OutOfRange("timer id"))
}

/// A [`Duration`] as milliseconds.
pub(super) fn duration_to_ms(duration: Duration, what: &'static str) -> Result<i64, StoreError> {
    i64::try_from(duration.as_millis()).map_err(|_| StoreError::OutOfRange(what))
}

/// Milliseconds read from the database as a [`Duration`]; negative values are an error.
pub(super) fn duration_from_ms(millis: i64, what: &'static str) -> Result<Duration, StoreError> {
    u64::try_from(millis)
        .map(Duration::from_millis)
        .map_err(|_| StoreError::OutOfRange(what))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_rejects_negative_and_huge_values() {
        assert_eq!(u32_from_sql(7, "count").unwrap(), 7);
        assert!(matches!(
            u32_from_sql(-1, "count"),
            Err(StoreError::OutOfRange("count"))
        ));
        assert!(matches!(
            u32_from_sql(i64::from(u32::MAX) + 1, "count"),
            Err(StoreError::OutOfRange("count"))
        ));
    }

    #[test]
    fn timer_ids_roundtrip_and_reject_out_of_range() {
        assert_eq!(timer_id_to_sql(TimerId(42)).unwrap(), 42);
        assert_eq!(timer_id_from_sql(42).unwrap(), TimerId(42));
        assert!(matches!(
            timer_id_to_sql(TimerId(u64::MAX)),
            Err(StoreError::OutOfRange("timer id"))
        ));
        assert!(matches!(
            timer_id_from_sql(-1),
            Err(StoreError::OutOfRange("timer id"))
        ));
    }

    #[test]
    fn durations_roundtrip_and_reject_out_of_range() {
        let duration = Duration::from_millis(1500);
        assert_eq!(duration_to_ms(duration, "duration").unwrap(), 1500);
        assert_eq!(duration_from_ms(1500, "duration").unwrap(), duration);
        assert!(matches!(
            duration_to_ms(Duration::MAX, "duration"),
            Err(StoreError::OutOfRange("duration"))
        ));
        assert!(matches!(
            duration_from_ms(-1, "duration"),
            Err(StoreError::OutOfRange("duration"))
        ));
    }
}
