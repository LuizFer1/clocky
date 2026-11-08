use std::time::Duration;

use chrono::{Datelike, FixedOffset, NaiveDate, Weekday};
use chrono_tz::America::{New_York, Sao_Paulo};

use super::*;
use crate::reports::test_support::{date, local};

fn hours(n: u64) -> Duration {
    Duration::from_hours(n)
}

#[test]
fn day_runs_from_local_midnight_to_the_next() {
    let period = Period::day(date(2026, 11, 4), &New_York).unwrap();
    assert_eq!(period.start(), local(&New_York, (2026, 11, 4), (0, 0)));
    assert_eq!(period.end(), local(&New_York, (2026, 11, 5), (0, 0)));
    assert_eq!(period.length(), hours(24));
}

#[test]
fn spring_forward_day_has_23_hours() {
    let period = Period::day(date(2026, 3, 8), &New_York).unwrap();
    assert_eq!(period.length(), hours(23));
}

#[test]
fn fall_back_day_has_25_hours() {
    let period = Period::day(date(2026, 11, 1), &New_York).unwrap();
    assert_eq!(period.length(), hours(25));
}

#[test]
fn missing_midnight_starts_the_day_at_the_first_valid_minute() {
    // Brazil started DST on 2018-11-04 at 00:00, jumping straight to 01:00.
    let period = Period::day(date(2018, 11, 4), &Sao_Paulo).unwrap();
    assert_eq!(period.start(), local(&Sao_Paulo, (2018, 11, 4), (1, 0)));
    assert_eq!(period.length(), hours(23));
    let before = Period::day(date(2018, 11, 3), &Sao_Paulo).unwrap();
    assert_eq!(before.end(), period.start());
}

#[test]
fn week_runs_from_monday_to_sunday_whatever_the_day() {
    let tz = FixedOffset::west_opt(3 * 3600).unwrap();
    for day in 2..=8 {
        let period = Period::week_of(date(2026, 11, day), &tz).unwrap();
        assert_eq!(
            period.start(),
            local(&tz, (2026, 11, 2), (0, 0)),
            "day {day}"
        );
        assert_eq!(period.end(), local(&tz, (2026, 11, 9), (0, 0)), "day {day}");
    }
}

#[test]
fn week_with_a_dst_change_keeps_local_midnights() {
    let period = Period::week_of(date(2026, 3, 8), &New_York).unwrap();
    assert_eq!(period.start(), local(&New_York, (2026, 3, 2), (0, 0)));
    assert_eq!(period.end(), local(&New_York, (2026, 3, 9), (0, 0)));
    assert_eq!(period.length(), hours(7 * 24 - 1));
}

#[test]
fn monday_of_handles_every_weekday() {
    assert_eq!(monday_of(date(2026, 11, 8)), Some(date(2026, 11, 2)));
    assert_eq!(monday_of(date(2026, 11, 2)), Some(date(2026, 11, 2)));
    assert_eq!(
        monday_of(date(2026, 11, 2)).unwrap().weekday(),
        Weekday::Mon
    );
}

#[test]
fn days_out_of_range_are_an_error() {
    assert!(matches!(
        Period::day(NaiveDate::MAX, &New_York),
        Err(ReportError::DateOutOfRange(_))
    ));
    assert!(matches!(
        Period::week_of(NaiveDate::MIN, &New_York),
        Err(ReportError::DateOutOfRange(_))
    ));
}

#[test]
fn custom_accepts_ordered_bounds_and_rejects_reversed_ones() {
    let (a, b) = (Timestamp::from_millis(10), Timestamp::from_millis(20));
    let period = Period::custom(a, b).unwrap();
    assert_eq!((period.start(), period.end()), (a, b));
    assert_eq!(Period::custom(a, a).unwrap().length(), Duration::ZERO);
    assert!(matches!(
        Period::custom(b, a),
        Err(ReportError::InvalidPeriod { start, end }) if start == b && end == a
    ));
}

#[test]
fn clip_keeps_only_the_overlap() {
    let ms = Timestamp::from_millis;
    let period = Period::custom(ms(100), ms(200)).unwrap();
    let seg = |start, end| Segment {
        start: ms(start),
        end: ms(end),
    };
    assert_eq!(period.clip(seg(50, 150)), Some(seg(100, 150)));
    assert_eq!(period.clip(seg(120, 180)), Some(seg(120, 180)));
    assert_eq!(period.clip(seg(150, 250)), Some(seg(150, 200)));
    assert_eq!(period.clip(seg(50, 250)), Some(seg(100, 200)));
    assert_eq!(period.clip(seg(50, 100)), None);
    assert_eq!(period.clip(seg(200, 250)), None);
}

#[test]
fn contains_is_half_open() {
    let ms = Timestamp::from_millis;
    let period = Period::custom(ms(100), ms(200)).unwrap();
    assert!(period.contains(ms(100)));
    assert!(period.contains(ms(199)));
    assert!(!period.contains(ms(200)));
    assert!(!period.contains(ms(99)));
}
