//! Run with pauses: the sequence of segments during which the timer was running.

use std::time::Duration;

use super::error::EngineError;
use crate::time::Timestamp;

/// Continuous segment during which a timer was running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Start of the segment.
    pub start: Timestamp,
    /// End of the segment.
    pub end: Timestamp,
}

impl Segment {
    /// Duration of the segment.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.end.since(self.start)
    }
}

/// Run of a timer: closed segments and, if running, the start of the open segment.
///
/// Invariant: segments in order, non-overlapping, each with `end >= start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    closed: Vec<Segment>,
    running_since: Option<Timestamp>,
}

impl Run {
    /// Starts a run, running from `now`.
    #[must_use]
    pub fn start(now: Timestamp) -> Self {
        Self {
            closed: Vec::new(),
            running_since: Some(now),
        }
    }

    /// Restores a run from its closed segments and, if running, the start of the open segment.
    ///
    /// # Errors
    /// [`EngineError::InvalidRun`] if a segment ends before it starts, the segments are out of
    /// order or overlap, or the open segment starts before the last closed one ends.
    pub fn from_parts(
        closed: Vec<Segment>,
        running_since: Option<Timestamp>,
    ) -> Result<Self, EngineError> {
        let ordered = closed.iter().all(|segment| segment.end >= segment.start)
            && closed.windows(2).all(|pair| pair[1].start >= pair[0].end);
        let open_after_last = match (closed.last(), running_since) {
            (Some(last), Some(since)) => since >= last.end,
            _ => true,
        };
        if !ordered || !open_after_last {
            return Err(EngineError::InvalidRun);
        }
        Ok(Self {
            closed,
            running_since,
        })
    }

    /// Closed segments, in order.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.closed
    }

    /// Start of the open segment, if running.
    #[must_use]
    pub fn running_since(&self) -> Option<Timestamp> {
        self.running_since
    }

    /// Whether it is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    /// Pauses, closing the open segment at `now`.
    ///
    /// # Errors
    /// [`EngineError::NotRunning`] if already paused.
    pub fn pause(&mut self, now: Timestamp) -> Result<(), EngineError> {
        let since = self.running_since.take().ok_or(EngineError::NotRunning)?;
        self.closed.push(Segment {
            start: since,
            end: now.max(since),
        });
        Ok(())
    }

    /// Resumes, opening a new segment at `now`.
    ///
    /// If the clock moved back to before the last pause, resumes from that pause (segments always
    /// stay in order and non-overlapping).
    ///
    /// # Errors
    /// [`EngineError::AlreadyRunning`] if already running.
    pub fn resume(&mut self, now: Timestamp) -> Result<(), EngineError> {
        if self.running_since.is_some() {
            return Err(EngineError::AlreadyRunning);
        }
        self.running_since = Some(self.closed.last().map_or(now, |last| now.max(last.end)));
        Ok(())
    }

    /// Effective time run up to `now`.
    #[must_use]
    pub fn elapsed(&self, now: Timestamp) -> Duration {
        let open = self
            .running_since
            .map_or(Duration::ZERO, |since| now.since(since));
        self.closed_total() + open
    }

    /// Instant at which the effective time reaches `target`, including when that happened inside
    /// an already closed segment; `None` if paused before reaching it.
    #[must_use]
    pub fn reaches_at(&self, target: Duration) -> Option<Timestamp> {
        let mut before = Duration::ZERO;
        for segment in &self.closed {
            let length = segment.duration();
            if before + length >= target {
                return Some(segment.start.plus(target.saturating_sub(before)));
            }
            before += length;
        }
        let since = self.running_since?;
        Some(since.plus(target.saturating_sub(before)))
    }

    /// Finishes at `end` and returns the segments up to `end`: the open one is closed at `end` and
    /// any time after `end` is discarded.
    #[must_use]
    pub fn finish(mut self, end: Timestamp) -> Vec<Segment> {
        if let Some(since) = self.running_since.take() {
            self.closed.push(Segment {
                start: since,
                end: end.max(since),
            });
        }
        self.closed
            .into_iter()
            .filter(|segment| segment.start <= end)
            .map(|segment| Segment {
                start: segment.start,
                end: segment.end.min(end),
            })
            .collect()
    }

    /// Finishes by user action at `now`. If the clock moved back to before the last segment,
    /// finishes at the end of that segment instead of discarding time already recorded.
    #[must_use]
    pub fn stop(self, now: Timestamp) -> Vec<Segment> {
        let last = self
            .running_since
            .or_else(|| self.closed.last().map(|segment| segment.end));
        let end = last.map_or(now, |last| now.max(last));
        self.finish(end)
    }

    fn closed_total(&self) -> Duration {
        self.closed.iter().map(Segment::duration).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, secs};

    #[test]
    fn running_elapsed_counts_from_start() {
        let run = Run::start(at(0));
        assert_eq!(run.elapsed(at(10)), secs(10));
        assert!(run.is_running());
    }

    #[test]
    fn pause_freezes_elapsed() {
        let mut run = Run::start(at(0));
        run.pause(at(10)).unwrap();
        assert_eq!(run.elapsed(at(50)), secs(10));
        assert!(!run.is_running());
    }

    #[test]
    fn resume_opens_a_new_segment() {
        let mut run = Run::start(at(0));
        run.pause(at(10)).unwrap();
        run.resume(at(20)).unwrap();
        assert_eq!(run.elapsed(at(25)), secs(15));
    }

    #[test]
    fn pause_when_paused_is_an_error() {
        let mut run = Run::start(at(0));
        run.pause(at(10)).unwrap();
        assert_eq!(run.pause(at(11)), Err(EngineError::NotRunning));
    }

    #[test]
    fn resume_when_running_is_an_error() {
        let mut run = Run::start(at(0));
        assert_eq!(run.resume(at(1)), Err(EngineError::AlreadyRunning));
    }

    #[test]
    fn finish_closes_the_open_segment() {
        let mut run = Run::start(at(0));
        run.pause(at(10)).unwrap();
        run.resume(at(20)).unwrap();
        assert_eq!(
            run.finish(at(30)),
            vec![
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
    fn finish_before_start_records_nothing() {
        let run = Run::start(at(10));
        assert_eq!(run.finish(at(5)), Vec::<Segment>::new());
    }

    #[test]
    fn reaches_at_accounts_for_pauses() {
        let mut run = Run::start(at(0));
        run.pause(at(10)).unwrap();
        run.resume(at(20)).unwrap();
        assert_eq!(run.reaches_at(secs(25)), Some(at(35)));
    }

    #[test]
    fn reaches_at_is_none_while_paused() {
        let mut run = Run::start(at(0));
        run.pause(at(10)).unwrap();
        assert_eq!(run.reaches_at(secs(25)), None);
    }

    #[test]
    fn segment_duration() {
        let segment = Segment {
            start: at(3),
            end: at(10),
        };
        assert_eq!(segment.duration(), secs(7));
    }

    #[test]
    fn reaches_at_finds_crossing_inside_closed_segments() {
        let mut run = Run::start(at(0));
        run.pause(at(70)).unwrap();
        assert_eq!(run.reaches_at(secs(60)), Some(at(60)));
        run.resume(at(100)).unwrap();
        assert_eq!(run.reaches_at(secs(60)), Some(at(60)));
    }

    #[test]
    fn finish_drops_time_after_end() {
        let mut run = Run::start(at(0));
        run.pause(at(70)).unwrap();
        run.resume(at(100)).unwrap();
        assert_eq!(
            run.finish(at(60)),
            vec![Segment {
                start: at(0),
                end: at(60)
            }]
        );
    }

    #[test]
    fn stop_with_clock_moved_back_keeps_recorded_time() {
        let mut run = Run::start(at(100));
        run.pause(at(110)).unwrap();
        assert_eq!(
            run.stop(at(50)),
            vec![Segment {
                start: at(100),
                end: at(110)
            }]
        );
    }

    #[test]
    fn resume_never_goes_back_before_last_pause() {
        let mut run = Run::start(at(100));
        run.pause(at(110)).unwrap();
        run.resume(at(50)).unwrap(); // the system clock moved back
        assert_eq!(
            run.finish(at(120)),
            vec![
                Segment {
                    start: at(100),
                    end: at(110)
                },
                Segment {
                    start: at(110),
                    end: at(120)
                },
            ]
        );
    }

    fn seg(start: i64, end: i64) -> Segment {
        Segment {
            start: at(start),
            end: at(end),
        }
    }

    #[test]
    fn from_parts_restores_a_run() {
        let mut run = Run::start(at(0));
        run.pause(at(10)).unwrap();
        run.resume(at(20)).unwrap();
        let restored = Run::from_parts(run.segments().to_vec(), run.running_since()).unwrap();
        assert_eq!(restored, run);
        assert_eq!(restored.segments(), &[seg(0, 10)]);
        assert_eq!(restored.running_since(), Some(at(20)));
    }

    #[test]
    fn from_parts_accepts_an_empty_paused_run() {
        let run = Run::from_parts(vec![], None).unwrap();
        assert!(!run.is_running());
        assert_eq!(run.elapsed(at(100)), Duration::ZERO);
    }

    #[test]
    fn from_parts_rejects_inverted_or_unordered_segments() {
        assert_eq!(
            Run::from_parts(vec![seg(10, 5)], None),
            Err(EngineError::InvalidRun)
        );
        assert_eq!(
            Run::from_parts(vec![seg(0, 10), seg(5, 20)], None),
            Err(EngineError::InvalidRun)
        );
        assert_eq!(
            Run::from_parts(vec![seg(20, 30), seg(0, 10)], None),
            Err(EngineError::InvalidRun)
        );
        assert_eq!(
            Run::from_parts(vec![seg(0, 10)], Some(at(5))),
            Err(EngineError::InvalidRun)
        );
        assert!(Run::from_parts(vec![seg(0, 10), seg(10, 20)], Some(at(20))).is_ok());
        assert_eq!(
            EngineError::InvalidRun.to_string(),
            "invalid run: segments must be ordered, non-overlapping and not inverted"
        );
    }
}
