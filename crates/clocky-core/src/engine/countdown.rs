//! Countdown timer: counts down from a duration.

use std::time::Duration;

use super::error::EngineError;
use super::run::Run;
use super::session::{CompletedSession, TimerKind};
use crate::time::Timestamp;

/// Countdown timer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Countdown {
    run: Run,
    target: Duration,
}

impl Countdown {
    /// Starts a countdown of `target` at `now`.
    #[must_use]
    pub fn start(now: Timestamp, target: Duration) -> Self {
        Self {
            run: Run::start(now),
            target,
        }
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

    /// Time remaining at `now` (never negative).
    #[must_use]
    pub fn remaining(&self, now: Timestamp) -> Duration {
        self.target.saturating_sub(self.run.elapsed(now))
    }

    /// Exact instant at which it came due, if it is already due by `now`.
    #[must_use]
    pub fn finished_at(&self, now: Timestamp) -> Option<Timestamp> {
        self.run.reaches_at(self.target).filter(|due| *due <= now)
    }

    /// Ends at `at` (due time or cancellation) and returns the session.
    ///
    /// When it comes due, pass `finished_at(now)` (not `now`), so that no time beyond the
    /// scheduled duration is recorded.
    #[must_use]
    pub fn finish(self, at: Timestamp) -> Option<CompletedSession> {
        CompletedSession::new(TimerKind::Countdown, self.run.finish(at))
    }

    /// Cancels before it comes due, by user action (does not discard time if the clock moved back).
    #[must_use]
    pub fn cancel(self, now: Timestamp) -> Option<CompletedSession> {
        CompletedSession::new(TimerKind::Countdown, self.run.stop(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, secs};

    #[test]
    fn remaining_decreases_and_never_goes_negative() {
        let cd = Countdown::start(at(0), secs(60));
        assert_eq!(cd.remaining(at(20)), secs(40));
        assert_eq!(cd.remaining(at(100)), Duration::ZERO);
    }

    #[test]
    fn pause_keeps_remaining() {
        let mut cd = Countdown::start(at(0), secs(60));
        cd.pause(at(20)).unwrap();
        assert!(!cd.is_running());
        assert_eq!(cd.remaining(at(50)), secs(40));
        cd.resume(at(50)).unwrap();
        assert_eq!(cd.remaining(at(60)), secs(30));
    }

    #[test]
    fn finished_at_reports_the_exact_due_time_even_when_late() {
        let cd = Countdown::start(at(0), secs(60));
        assert_eq!(cd.finished_at(at(59)), None);
        assert_eq!(cd.finished_at(at(60)), Some(at(60)));
        // The app was closed: on reopening, the due time is still 60s.
        assert_eq!(cd.finished_at(at(5000)), Some(at(60)));
    }

    #[test]
    fn finished_at_accounts_for_pauses() {
        let mut cd = Countdown::start(at(0), secs(60));
        cd.pause(at(20)).unwrap();
        cd.resume(at(30)).unwrap();
        assert_eq!(cd.finished_at(at(1000)), Some(at(70)));
    }

    #[test]
    fn paused_countdown_never_finishes() {
        let mut cd = Countdown::start(at(0), secs(60));
        cd.pause(at(20)).unwrap();
        assert_eq!(cd.finished_at(at(1000)), None);
    }

    #[test]
    fn finish_at_due_time_records_full_duration() {
        let cd = Countdown::start(at(0), secs(60));
        let session = cd.finish(at(60)).unwrap();
        assert_eq!(session.kind(), TimerKind::Countdown);
        assert_eq!(
            (session.effective(), session.ended_at()),
            (secs(60), at(60))
        );
    }

    #[test]
    fn finish_early_records_partial_time() {
        let cd = Countdown::start(at(0), secs(60));
        assert_eq!(cd.finish(at(15)).unwrap().effective(), secs(15));
    }

    #[test]
    fn cancel_records_partial_time() {
        let cd = Countdown::start(at(0), secs(60));
        assert_eq!(cd.cancel(at(15)).unwrap().effective(), secs(15));
    }

    #[test]
    fn pausing_after_due_time_still_finishes_at_due_time() {
        let mut cd = Countdown::start(at(0), secs(60));
        cd.pause(at(70)).unwrap(); // paused between two ticks, after the due time
        assert_eq!(cd.finished_at(at(70)), Some(at(60)));
        let session = cd.finish(at(60)).unwrap();
        assert_eq!(
            (session.effective(), session.ended_at()),
            (secs(60), at(60))
        );
    }
}
