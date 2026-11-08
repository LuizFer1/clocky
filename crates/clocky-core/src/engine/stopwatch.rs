//! Stopwatch: counts up until stopped.

use std::time::Duration;

use super::error::EngineError;
use super::run::Run;
use super::session::{CompletedSession, TimerKind};
use crate::time::Timestamp;

/// Stopwatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopwatch {
    run: Run,
}

impl Stopwatch {
    /// Starts a stopwatch at `now`.
    #[must_use]
    pub fn start(now: Timestamp) -> Self {
        Self {
            run: Run::start(now),
        }
    }

    /// Restores a stopwatch from its run.
    #[must_use]
    pub fn from_parts(run: Run) -> Self {
        Self { run }
    }

    /// The run: segments and, if running, the start of the open one.
    #[must_use]
    pub fn run(&self) -> &Run {
        &self.run
    }

    /// Whether it is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.run.is_running()
    }

    /// Pauses.
    ///
    /// # Errors
    /// [`EngineError::NotRunning`] if already paused.
    pub fn pause(&mut self, now: Timestamp) -> Result<(), EngineError> {
        self.run.pause(now)
    }

    /// Resumes.
    ///
    /// # Errors
    /// [`EngineError::AlreadyRunning`] if already running.
    pub fn resume(&mut self, now: Timestamp) -> Result<(), EngineError> {
        self.run.resume(now)
    }

    /// Effective time up to `now`.
    #[must_use]
    pub fn elapsed(&self, now: Timestamp) -> Duration {
        self.run.elapsed(now)
    }

    /// Has it run longer than `limit`? Used for the forgotten-stopwatch warning.
    #[must_use]
    pub fn exceeds(&self, now: Timestamp, limit: Duration) -> bool {
        self.elapsed(now) > limit
    }

    /// Stops and returns the session (`None` if nothing was run).
    #[must_use]
    pub fn stop(self, now: Timestamp) -> Option<CompletedSession> {
        CompletedSession::new(TimerKind::Stopwatch, self.run.stop(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::run::Segment;
    use crate::test_util::{at, secs};

    #[test]
    fn counts_up_and_freezes_when_paused() {
        let mut sw = Stopwatch::start(at(0));
        assert_eq!(sw.elapsed(at(30)), secs(30));
        sw.pause(at(30)).unwrap();
        assert!(!sw.is_running());
        assert_eq!(sw.elapsed(at(100)), secs(30));
        sw.resume(at(100)).unwrap();
        assert_eq!(sw.elapsed(at(110)), secs(40));
    }

    #[test]
    fn exceeds_detects_a_forgotten_stopwatch() {
        let eight_hours = secs(8 * 3600);
        let sw = Stopwatch::start(at(0));
        assert!(!sw.exceeds(at(8 * 3600), eight_hours));
        assert!(sw.exceeds(at(8 * 3600 + 1), eight_hours));
    }

    #[test]
    fn stop_returns_the_session() {
        let mut sw = Stopwatch::start(at(0));
        sw.pause(at(10)).unwrap();
        sw.resume(at(20)).unwrap();
        let session = sw.stop(at(30)).unwrap();
        assert_eq!(session.kind(), TimerKind::Stopwatch);
        assert_eq!(
            session.segments(),
            &[
                Segment {
                    start: at(0),
                    end: at(10)
                },
                Segment {
                    start: at(20),
                    end: at(30)
                },
            ]
        );
    }

    #[test]
    fn stop_with_clock_moved_back_keeps_recorded_time() {
        let mut sw = Stopwatch::start(at(0));
        sw.pause(at(3600)).unwrap();
        let session = sw.stop(at(1800)).unwrap();
        assert_eq!(session.effective(), secs(3600));
    }

    #[test]
    fn from_parts_restores_the_run() {
        let mut sw = Stopwatch::start(at(0));
        sw.pause(at(10)).unwrap();
        let restored = Stopwatch::from_parts(sw.run().clone());
        assert_eq!(restored, sw);
        assert_eq!(restored.elapsed(at(99)), secs(10));
    }

    #[test]
    fn stop_right_away_records_nothing() {
        let sw = Stopwatch::start(at(0));
        assert!(sw.stop(at(0)).is_none());
    }
}
