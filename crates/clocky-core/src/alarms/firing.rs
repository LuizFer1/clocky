//! How late an alarm rings (decision 4 of the spec) and snooze.

use std::time::Duration;

use super::error::AlarmError;
use crate::time::Timestamp;

/// Delay up to which an alarm still counts as on time.
///
/// The app checks alarms on a tick, so it always notices a due alarm a little late. One
/// minute matches the minute precision of alarms and the 60 s suspend threshold of the
/// timer engine (spec, section 4).
pub const ON_TIME_GRACE: Duration = Duration::from_mins(1);

/// Delay up to which a late alarm still rings, marked as late (decision 4).
pub const LATE_TOLERANCE: Duration = Duration::from_mins(30);

/// How an alarm that came due should be presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firing {
    /// Rings normally.
    OnTime,
    /// Rings, marked as late ("atrasado").
    Late {
        /// How late it rings.
        by: Duration,
    },
    /// Does not ring: only an in-app notice ("alarme perdido às HH:MM").
    Missed,
}

/// Classifies an alarm due at `due` that is noticed at `now`.
///
/// - Up to [`ON_TIME_GRACE`] late (or `now` before `due`): [`Firing::OnTime`].
/// - Up to [`LATE_TOLERANCE`] late, inclusive: [`Firing::Late`].
/// - More than that: [`Firing::Missed`].
#[must_use]
pub fn classify(due: Timestamp, now: Timestamp) -> Firing {
    let late = now.since(due);
    if late <= ON_TIME_GRACE {
        Firing::OnTime
    } else if late <= LATE_TOLERANCE {
        Firing::Late { by: late }
    } else {
        Firing::Missed
    }
}

/// The instant at which an alarm snoozed at `now` for `minutes` rings again.
///
/// # Errors
/// [`AlarmError::InvalidSnooze`] if `minutes` is zero.
pub fn snooze_until(now: Timestamp, minutes: u32) -> Result<Timestamp, AlarmError> {
    if minutes == 0 {
        return Err(AlarmError::InvalidSnooze);
    }
    Ok(now.plus(Duration::from_mins(u64::from(minutes))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, mins, secs};

    fn late_by(due: Timestamp, by: Duration) -> Firing {
        classify(due, due.plus(by))
    }

    #[test]
    fn noticed_exactly_at_the_due_time_is_on_time() {
        assert_eq!(classify(at(1000), at(1000)), Firing::OnTime);
    }

    #[test]
    fn noticed_before_the_due_time_is_on_time() {
        assert_eq!(classify(at(1000), at(990)), Firing::OnTime);
    }

    #[test]
    fn up_to_one_minute_late_is_on_time() {
        assert_eq!(
            late_by(at(1000), Duration::from_millis(200)),
            Firing::OnTime
        );
        assert_eq!(late_by(at(1000), ON_TIME_GRACE), Firing::OnTime);
    }

    #[test]
    fn just_over_one_minute_late_is_late() {
        assert_eq!(late_by(at(1000), secs(61)), Firing::Late { by: secs(61) });
    }

    #[test]
    fn exactly_thirty_minutes_late_still_rings_as_late() {
        assert_eq!(late_by(at(1000), mins(30)), Firing::Late { by: mins(30) });
    }

    #[test]
    fn thirty_minutes_and_one_second_late_is_missed() {
        assert_eq!(late_by(at(1000), mins(30) + secs(1)), Firing::Missed);
    }

    #[test]
    fn thirty_minutes_and_one_millisecond_late_is_missed() {
        let by = mins(30) + Duration::from_millis(1);
        assert_eq!(late_by(at(1000), by), Firing::Missed);
    }

    #[test]
    fn days_late_is_missed() {
        assert_eq!(late_by(at(1000), mins(3 * 24 * 60)), Firing::Missed);
    }

    #[test]
    fn snooze_rings_again_after_the_given_minutes() {
        assert_eq!(snooze_until(at(1000), 5), Ok(at(1000).plus(mins(5))));
        assert_eq!(snooze_until(at(1000), 1), Ok(at(1060)));
    }

    #[test]
    fn snooze_of_zero_minutes_is_rejected() {
        assert_eq!(snooze_until(at(1000), 0), Err(AlarmError::InvalidSnooze));
    }

    #[test]
    fn snooze_saturates_instead_of_overflowing() {
        let max = Timestamp::from_millis(i64::MAX);
        assert_eq!(snooze_until(max, u32::MAX), Ok(max));
    }

    #[test]
    fn invalid_snooze_error_explains_the_minimum() {
        let message = AlarmError::InvalidSnooze.to_string();
        assert!(message.contains("at least 1 minute"), "{message}");
    }
}
