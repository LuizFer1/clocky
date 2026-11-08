//! Checked conversions between Rust types and SQLite integers (always `i64`, in milliseconds).

use super::StoreError;

/// An `INTEGER` read from the database as `u32` (counts, versions, rounds).
pub(super) fn u32_from_sql(value: i64, what: &'static str) -> Result<u32, StoreError> {
    u32::try_from(value).map_err(|_| StoreError::OutOfRange(what))
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
}
