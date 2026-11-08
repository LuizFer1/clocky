//! Completed session, ready to be recorded.

use std::time::Duration;

use super::run::Segment;
use crate::time::Timestamp;

/// Kind of timer that produced a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
    /// Focus phase of a pomodoro.
    Pomodoro,
    /// Stopwatch.
    Stopwatch,
    /// Countdown timer.
    Countdown,
}

/// Completed session: the segments during which the timer ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedSession {
    kind: TimerKind,
    segments: Vec<Segment>,
    started_at: Timestamp,
    ended_at: Timestamp,
    effective: Duration,
}

impl CompletedSession {
    /// Creates the session; returns `None` if no time was run.
    #[must_use]
    pub fn new(kind: TimerKind, segments: Vec<Segment>) -> Option<Self> {
        let started_at = segments.first()?.start;
        let ended_at = segments.last()?.end;
        let effective: Duration = segments.iter().map(Segment::duration).sum();
        if effective.is_zero() {
            return None;
        }
        Some(Self {
            kind,
            segments,
            started_at,
            ended_at,
            effective,
        })
    }

    /// Kind of timer.
    #[must_use]
    pub fn kind(&self) -> TimerKind {
        self.kind
    }

    /// Segments that ran, in order.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Start of the first segment.
    #[must_use]
    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// End of the last segment.
    #[must_use]
    pub fn ended_at(&self) -> Timestamp {
        self.ended_at
    }

    /// Effective time (sum of the segments, excluding pauses).
    #[must_use]
    pub fn effective(&self) -> Duration {
        self.effective
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, secs};

    #[test]
    fn new_rejects_sessions_without_time() {
        assert!(CompletedSession::new(TimerKind::Stopwatch, vec![]).is_none());
        let empty = vec![Segment {
            start: at(5),
            end: at(5),
        }];
        assert!(CompletedSession::new(TimerKind::Stopwatch, empty).is_none());
    }

    #[test]
    fn bounds_and_effective_time_skip_pauses() {
        let segments = vec![
            Segment {
                start: at(0),
                end: at(10),
            },
            Segment {
                start: at(20),
                end: at(30),
            },
        ];
        let session = CompletedSession::new(TimerKind::Countdown, segments.clone()).unwrap();
        assert_eq!(session.kind(), TimerKind::Countdown);
        assert_eq!(session.started_at(), at(0));
        assert_eq!(session.ended_at(), at(30));
        assert_eq!(session.effective(), secs(20));
        assert_eq!(session.segments(), segments.as_slice());
    }
}
