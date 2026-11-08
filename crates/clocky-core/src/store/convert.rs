//! Checked conversions between Rust types and SQLite integers (always `i64`, in milliseconds).

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
}
