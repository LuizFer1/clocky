//! Time source. The core never reads the clock directly (spec, section 10.3).

use std::cell::Cell;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::time::Timestamp;

/// Source of the current instant.
pub trait Clock {
    /// The current instant.
    fn now(&self) -> Timestamp;
}

/// The real system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[expect(
        clippy::disallowed_methods,
        reason = "the only place allowed to read the system clock"
    )]
    fn now(&self) -> Timestamp {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        Timestamp::from_millis(millis)
    }
}

/// A controllable clock, for tests.
#[derive(Debug)]
pub struct FakeClock {
    now: Cell<Timestamp>,
}

impl FakeClock {
    /// Creates a clock stopped at `start`.
    #[must_use]
    pub fn new(start: Timestamp) -> Self {
        Self {
            now: Cell::new(start),
        }
    }

    /// Advances the clock.
    pub fn advance(&self, by: Duration) {
        self.now.set(self.now.get().plus(by));
    }

    /// Sets the current instant.
    pub fn set(&self, to: Timestamp) {
        self.now.set(to);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, secs};

    #[test]
    fn fake_clock_starts_at_given_instant() {
        let clock = FakeClock::new(at(100));
        assert_eq!(clock.now(), at(100));
    }

    #[test]
    fn fake_clock_advances() {
        let clock = FakeClock::new(at(100));
        clock.advance(secs(30));
        assert_eq!(clock.now(), at(130));
    }

    #[test]
    fn fake_clock_can_be_set() {
        let clock = FakeClock::new(at(100));
        clock.set(at(5));
        assert_eq!(clock.now(), at(5));
    }

    #[test]
    fn system_clock_is_after_2020() {
        let jan_2020 = Timestamp::from_millis(1_577_836_800_000);
        assert!(SystemClock.now() > jan_2020);
    }
}
