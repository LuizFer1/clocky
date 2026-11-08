//! Clocky time types.

use std::time::Duration;

/// Absolute instant in milliseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Creates an instant from milliseconds since the Unix epoch.
    #[must_use]
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// Milliseconds since the Unix epoch.
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// The instant shifted forward by `duration` (saturates instead of overflowing).
    #[must_use]
    pub fn plus(self, duration: Duration) -> Self {
        let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
        Self(self.0.saturating_add(millis))
    }

    /// Time elapsed since `earlier`; zero if `earlier` is later.
    #[must_use]
    pub fn since(self, earlier: Self) -> Duration {
        let diff = self.0.saturating_sub(earlier.0);
        u64::try_from(diff).map_or(Duration::ZERO, Duration::from_millis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, secs};

    #[test]
    fn millis_roundtrip() {
        assert_eq!(Timestamp::from_millis(1234).as_millis(), 1234);
    }

    #[test]
    fn plus_moves_forward() {
        assert_eq!(at(10).plus(secs(5)), at(15));
    }

    #[test]
    fn since_returns_elapsed() {
        assert_eq!(at(15).since(at(10)), secs(5));
    }

    #[test]
    fn since_a_later_instant_is_zero() {
        assert_eq!(at(10).since(at(15)), Duration::ZERO);
    }

    #[test]
    fn plus_saturates_instead_of_overflowing() {
        let max = Timestamp::from_millis(i64::MAX);
        assert_eq!(max.plus(secs(1)), max);
    }

    #[test]
    fn plus_saturates_huge_durations() {
        assert_eq!(at(0).plus(Duration::MAX), Timestamp::from_millis(i64::MAX));
    }

    #[test]
    fn since_saturates_at_extremes() {
        let span = Timestamp::from_millis(i64::MAX).since(Timestamp::from_millis(i64::MIN));
        assert_eq!(
            span,
            Duration::from_millis(u64::try_from(i64::MAX).unwrap())
        );
    }
}
