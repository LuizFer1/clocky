//! Report periods: half-open ranges of instants built from local dates.

use std::time::Duration;

use chrono::{Datelike, Days, NaiveDate, NaiveTime, TimeDelta, TimeZone};

use super::ReportError;
use crate::engine::run::Segment;
use crate::time::Timestamp;

/// A half-open range of instants `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    start: Timestamp,
    end: Timestamp,
}

impl Period {
    /// The local day `date` in the zone `tz`: from its local midnight to the next one.
    ///
    /// Days with a daylight saving change last 23 or 25 hours. If midnight does not exist (the
    /// clock jumps over it), the day starts at the first valid local minute after it; if it
    /// happens twice, at the earliest one.
    ///
    /// # Errors
    /// [`ReportError::DateOutOfRange`] if either midnight cannot be represented.
    pub fn day<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> Result<Self, ReportError> {
        let next = date.succ_opt().ok_or(ReportError::DateOutOfRange(date))?;
        Ok(Self {
            start: local_midnight(date, tz)?,
            end: local_midnight(next, tz)?,
        })
    }

    /// The local week (Monday to Sunday) that contains `date`, in the zone `tz`.
    ///
    /// # Errors
    /// [`ReportError::DateOutOfRange`] if the Monday or the next Monday cannot be represented.
    pub fn week_of<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> Result<Self, ReportError> {
        let monday = monday_of(date).ok_or(ReportError::DateOutOfRange(date))?;
        let next = monday
            .checked_add_days(Days::new(DAYS_PER_WEEK))
            .ok_or(ReportError::DateOutOfRange(date))?;
        Ok(Self {
            start: local_midnight(monday, tz)?,
            end: local_midnight(next, tz)?,
        })
    }

    /// A custom range `[start, end)`; `start == end` is an empty period.
    ///
    /// # Errors
    /// [`ReportError::InvalidPeriod`] if `end` is before `start`.
    pub fn custom(start: Timestamp, end: Timestamp) -> Result<Self, ReportError> {
        if end < start {
            return Err(ReportError::InvalidPeriod { start, end });
        }
        Ok(Self { start, end })
    }

    /// First instant of the period.
    #[must_use]
    pub fn start(self) -> Timestamp {
        self.start
    }

    /// First instant after the period.
    #[must_use]
    pub fn end(self) -> Timestamp {
        self.end
    }

    /// Length of the period.
    #[must_use]
    pub fn length(self) -> Duration {
        self.end.since(self.start)
    }

    /// Whether `at` is inside the period.
    #[must_use]
    pub fn contains(self, at: Timestamp) -> bool {
        self.start <= at && at < self.end
    }

    /// The part of `segment` inside the period, if it has a duration.
    #[must_use]
    pub fn clip(self, segment: Segment) -> Option<Segment> {
        let start = segment.start.max(self.start);
        let end = segment.end.min(self.end);
        (end > start).then_some(Segment { start, end })
    }
}

/// Days in a week.
pub(crate) const DAYS_PER_WEEK: u64 = 7;

/// Longest run of nonexistent local minutes that is skipped when looking for midnight. Real
/// gaps last at most a day (Samoa skipped 2011-12-30); two days leave a margin.
const MAX_GAP_MINUTES: i64 = 2 * 24 * 60;

/// The Monday of the week that contains `date`.
pub(crate) fn monday_of(date: NaiveDate) -> Option<NaiveDate> {
    let back = date.weekday().num_days_from_monday();
    date.checked_sub_days(Days::new(u64::from(back)))
}

/// The instant of local midnight on `date`: the earliest one in a fold, the first valid minute
/// after it in a gap.
fn local_midnight<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> Result<Timestamp, ReportError> {
    let midnight = date.and_time(NaiveTime::MIN);
    (0..=MAX_GAP_MINUTES)
        .map_while(|minutes| midnight.checked_add_signed(TimeDelta::minutes(minutes)))
        .find_map(|candidate| tz.from_local_datetime(&candidate).earliest())
        .map(|start| Timestamp::from_millis(start.timestamp_millis()))
        .ok_or(ReportError::DateOutOfRange(date))
}

#[cfg(test)]
mod tests;
