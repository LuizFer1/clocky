//! Bridge between the core and the UI, driven by a 1 Hz `slint::Timer` on the UI thread
//! (spec, section 2). The core has nothing to report in M5: each tick reads the clock and
//! publishes the number of active items (always 0) to the dock badge. M6 replaces this with
//! `core.tick(now)` and the real state.

use std::time::Duration;

use clocky_core::clock::Clock;
use clocky_core::time::Timestamp;

/// Tick period.
pub const TICK: Duration = Duration::from_secs(1);

/// What the UI shows after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub now: Timestamp,
    /// Running timers and pending alarms (the dock badge).
    pub active_count: i32,
}

pub struct Bridge<C: Clock> {
    clock: C,
}

impl<C: Clock> Bridge<C> {
    pub fn new(clock: C) -> Self {
        Self { clock }
    }

    pub fn tick(&self) -> Snapshot {
        Snapshot {
            now: self.clock.now(),
            active_count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use clocky_core::clock::FakeClock;

    use super::*;

    #[test]
    fn tick_reads_the_injected_clock() {
        let clock = FakeClock::new(Timestamp::from_millis(5_000));
        let bridge = Bridge::new(clock);
        assert_eq!(bridge.tick().now, Timestamp::from_millis(5_000));
    }

    #[test]
    fn nothing_is_active_before_m6() {
        let bridge = Bridge::new(FakeClock::new(Timestamp::from_millis(0)));
        assert_eq!(bridge.tick().active_count, 0);
    }
}
