//! Next ring of an alarm, computed in local time.

use chrono::{Datelike, Days, NaiveDateTime, TimeDelta, TimeZone};

use super::alarm_time::AlarmTime;
use super::weekdays::Weekdays;
use crate::time::Timestamp;

/// The first instant strictly after `after` at which the alarm rings in the zone `tz`.
///
/// - [`Weekdays::ONCE`]: the next `time`, today if it is still ahead, otherwise tomorrow.
/// - Otherwise: `time` on the next day in `repeat`, matched on the local date.
///
/// Daylight saving time (spec, section 5):
/// - **Gap**: if `time` does not exist on a day, the alarm rings at the first valid minute
///   after it (for example 02:30 on a day that jumps from 02:00 to 03:00 rings at 03:00).
/// - **Fold**: if `time` happens twice on a day, the alarm rings only at the first one.
///
/// Returns `None` only when no such instant can be represented (out of chrono's range).
#[must_use]
pub fn next_fire<Tz: TimeZone>(
    time: AlarmTime,
    repeat: Weekdays,
    after: Timestamp,
    tz: &Tz,
) -> Option<Timestamp> {
    let today = tz
        .timestamp_millis_opt(after.as_millis())
        .single()?
        .date_naive();
    (0..=DAYS_AHEAD)
        .map_while(|offset| today.checked_add_days(Days::new(offset)))
        .filter(|date| repeat.is_once() || repeat.contains(date.weekday()))
        .filter_map(|date| resolve_local(date.and_time(time.to_naive_time()), tz))
        .find(|fire| *fire > after)
}

/// Days scanned after today: a weekly alarm whose day is today but already passed rings
/// again in 7 days.
const DAYS_AHEAD: u64 = 7;

/// Longest run of nonexistent local minutes that is skipped. Real gaps last at most a day
/// (Samoa skipped 2011-12-30); two days leave a margin.
const MAX_GAP_MINUTES: i64 = 2 * 24 * 60;

/// The instant of the local time `local`: the first occurrence in a fold, the first valid
/// minute after it in a gap.
fn resolve_local<Tz: TimeZone>(local: NaiveDateTime, tz: &Tz) -> Option<Timestamp> {
    (0..=MAX_GAP_MINUTES)
        .map_while(|minutes| local.checked_add_signed(TimeDelta::minutes(minutes)))
        .find_map(|candidate| tz.from_local_datetime(&candidate).earliest())
        .map(|fire| Timestamp::from_millis(fire.timestamp_millis()))
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, Utc, Weekday};
    use chrono_tz::America::{New_York, Sao_Paulo};
    use chrono_tz::Pacific::Apia;

    use super::*;
    use crate::alarms::test_support::{brt, nov, time, utc};

    fn millis_before(ts: Timestamp, millis: i64) -> Timestamp {
        Timestamp::from_millis(ts.as_millis() - millis)
    }

    #[test]
    fn once_rings_today_when_still_ahead() {
        let tz = brt();
        let fire = next_fire(time(7, 0), Weekdays::ONCE, nov(&tz, 2, 6, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 2, 7, 0)));
    }

    #[test]
    fn once_rings_tomorrow_when_already_passed() {
        let tz = brt();
        let fire = next_fire(time(7, 0), Weekdays::ONCE, nov(&tz, 2, 8, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 3, 7, 0)));
    }

    #[test]
    fn after_exactly_at_the_fire_time_gives_the_next_one() {
        let tz = brt();
        let at_seven = nov(&tz, 2, 7, 0);
        let once = next_fire(time(7, 0), Weekdays::ONCE, at_seven, &tz);
        let daily = next_fire(time(7, 0), Weekdays::EVERY_DAY, at_seven, &tz);
        assert_eq!(once, Some(nov(&tz, 3, 7, 0)));
        assert_eq!(daily, Some(nov(&tz, 3, 7, 0)));
    }

    #[test]
    fn one_millisecond_before_the_fire_time_gives_that_one() {
        let tz = brt();
        let just_before = millis_before(nov(&tz, 2, 7, 0), 1);
        let fire = next_fire(time(7, 0), Weekdays::ONCE, just_before, &tz);
        assert_eq!(fire, Some(nov(&tz, 2, 7, 0)));
    }

    #[test]
    fn every_day_rings_the_next_day() {
        let tz = brt();
        let fire = next_fire(time(7, 0), Weekdays::EVERY_DAY, nov(&tz, 8, 9, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 9, 7, 0)));
    }

    #[test]
    fn weekdays_skip_the_weekend() {
        let tz = brt();
        // Friday the 6th, after the alarm: the next one is Monday the 9th.
        let fire = next_fire(time(7, 0), Weekdays::WEEKDAYS, nov(&tz, 6, 7, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 9, 7, 0)));
        // Saturday: also Monday.
        let fire = next_fire(time(7, 0), Weekdays::WEEKDAYS, nov(&tz, 7, 6, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 9, 7, 0)));
    }

    #[test]
    fn weekdays_ring_the_next_weekday_midweek() {
        let tz = brt();
        let fire = next_fire(time(7, 0), Weekdays::WEEKDAYS, nov(&tz, 3, 8, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 4, 7, 0)));
    }

    #[test]
    fn weekend_skips_to_saturday_then_sunday() {
        let tz = brt();
        let fire = next_fire(time(9, 30), Weekdays::WEEKEND, nov(&tz, 2, 10, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 7, 9, 30)));
        let fire = next_fire(time(9, 30), Weekdays::WEEKEND, nov(&tz, 7, 9, 30), &tz);
        assert_eq!(fire, Some(nov(&tz, 8, 9, 30)));
    }

    #[test]
    fn single_day_rings_later_the_same_day() {
        let tz = brt();
        let wed = Weekdays::single(Weekday::Wed);
        let fire = next_fire(time(7, 0), wed, nov(&tz, 4, 6, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 4, 7, 0)));
    }

    #[test]
    fn single_day_waits_a_full_week_once_passed() {
        let tz = brt();
        let wed = Weekdays::single(Weekday::Wed);
        let fire = next_fire(time(7, 0), wed, nov(&tz, 4, 7, 0), &tz);
        assert_eq!(fire, Some(nov(&tz, 11, 7, 0)));
    }

    #[test]
    fn wraps_past_midnight_into_the_next_day() {
        let tz = brt();
        let fire = next_fire(time(0, 15), Weekdays::ONCE, nov(&tz, 2, 23, 50), &tz);
        assert_eq!(fire, Some(nov(&tz, 3, 0, 15)));
    }

    #[test]
    fn last_minute_of_the_day_wraps_to_tomorrow_once_passed() {
        let tz = brt();
        let after = nov(&tz, 2, 23, 59);
        let fire = next_fire(time(23, 59), Weekdays::EVERY_DAY, after, &tz);
        assert_eq!(fire, Some(nov(&tz, 3, 23, 59)));
    }

    #[test]
    fn weekday_is_matched_on_the_local_date_not_the_utc_date() {
        let tz = brt();
        // Monday 22:00 in Brasília is already Tuesday 01:00 UTC.
        let after = nov(&tz, 2, 22, 0);
        let mon = Weekdays::single(Weekday::Mon);
        assert_eq!(
            next_fire(time(23, 0), mon, after, &tz),
            Some(nov(&tz, 2, 23, 0))
        );
    }

    #[test]
    fn works_in_utc() {
        let fire = next_fire(time(7, 0), Weekdays::ONCE, utc(2026, 11, 2, 6, 0), &Utc);
        assert_eq!(fire, Some(utc(2026, 11, 2, 7, 0)));
    }

    #[test]
    fn dst_gap_rings_at_the_first_valid_minute_after() {
        // New York, 2026-03-08 (Sunday): 02:00 EST jumps to 03:00 EDT (07:00 UTC).
        let after = utc(2026, 3, 8, 5, 0); // 00:00 EST
        let gap = utc(2026, 3, 8, 7, 0);
        for (hour, minute) in [(2, 0), (2, 30), (2, 59)] {
            let fire = next_fire(time(hour, minute), Weekdays::EVERY_DAY, after, &New_York);
            assert_eq!(fire, Some(gap), "{hour:02}:{minute:02}");
        }
    }

    #[test]
    fn dst_gap_on_a_repeating_day_still_rings_that_day() {
        let sun = Weekdays::single(Weekday::Sun);
        let after = utc(2026, 3, 7, 12, 0);
        let fire = next_fire(time(2, 30), sun, after, &New_York);
        assert_eq!(fire, Some(utc(2026, 3, 8, 7, 0)));
    }

    #[test]
    fn after_the_dst_gap_the_next_day_is_normal_again() {
        let after = utc(2026, 3, 8, 7, 0); // the 02:30 alarm rang at 03:00 EDT
        let fire = next_fire(time(2, 30), Weekdays::EVERY_DAY, after, &New_York);
        assert_eq!(fire, Some(utc(2026, 3, 9, 6, 30))); // 02:30 EDT
    }

    #[test]
    fn time_just_after_the_dst_gap_is_unaffected() {
        let after = utc(2026, 3, 8, 5, 0);
        let fire = next_fire(time(3, 0), Weekdays::ONCE, after, &New_York);
        assert_eq!(fire, Some(utc(2026, 3, 8, 7, 0)));
    }

    #[test]
    fn dst_fold_rings_at_the_first_occurrence() {
        // New York, 2026-11-01 (Sunday): 01:00-01:59 happens twice,
        // first in EDT (05:xx UTC), then in EST (06:xx UTC).
        let after = utc(2026, 11, 1, 4, 0); // 00:00 EDT
        let fire = next_fire(time(1, 30), Weekdays::EVERY_DAY, after, &New_York);
        assert_eq!(fire, Some(utc(2026, 11, 1, 5, 30)));
    }

    #[test]
    fn dst_fold_never_rings_the_second_occurrence() {
        let next_day = Some(utc(2026, 11, 2, 6, 30)); // 01:30 EST on Monday
        // Exactly at the first 01:30, and between the two 01:30s.
        for after in [utc(2026, 11, 1, 5, 30), utc(2026, 11, 1, 6, 0)] {
            let daily = next_fire(time(1, 30), Weekdays::EVERY_DAY, after, &New_York);
            let once = next_fire(time(1, 30), Weekdays::ONCE, after, &New_York);
            assert_eq!(daily, next_day);
            assert_eq!(once, next_day);
        }
    }

    #[test]
    fn dst_gap_at_midnight_rings_at_the_first_valid_minute() {
        // São Paulo, 2018-11-04: 00:00 (UTC-3) jumped to 01:00 (UTC-2, 03:00 UTC).
        let after = utc(2018, 11, 4, 2, 30); // 23:30 on the 3rd
        let fire = next_fire(time(0, 0), Weekdays::EVERY_DAY, after, &Sao_Paulo);
        assert_eq!(fire, Some(utc(2018, 11, 4, 3, 0)));
    }

    #[test]
    fn a_skipped_day_rings_at_the_first_minute_of_the_next_one() {
        // Samoa skipped 2011-12-30 entirely: the 29th at 23:59 (UTC-10)
        // was followed by the 31st at 00:00 (UTC+14).
        let after = utc(2011, 12, 29, 18, 0); // 08:00 on the 29th
        let fire = next_fire(time(7, 0), Weekdays::ONCE, after, &Apia);
        assert_eq!(fire, Some(utc(2011, 12, 30, 10, 0)));
    }

    #[test]
    fn out_of_range_instants_have_no_next_fire() {
        for millis in [i64::MAX, i64::MIN] {
            let after = Timestamp::from_millis(millis);
            assert_eq!(
                next_fire(time(7, 0), Weekdays::EVERY_DAY, after, &Utc),
                None
            );
        }
    }

    #[test]
    fn the_last_representable_day_has_no_tomorrow() {
        let last = NaiveDate::MAX.and_hms_opt(23, 59, 30).unwrap().and_utc();
        let after = Timestamp::from_millis(last.timestamp_millis());
        assert_eq!(next_fire(time(23, 59), Weekdays::ONCE, after, &Utc), None);
    }
}
