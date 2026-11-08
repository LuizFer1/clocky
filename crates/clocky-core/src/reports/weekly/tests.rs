use std::time::Duration;

use chrono::{Datelike, FixedOffset, Weekday};
use chrono_tz::America::New_York;

use super::*;
use crate::domain::{ProjectId, TaskId};
use crate::reports::test_support::{date, local, min, record};
use crate::time::Timestamp;

fn hours(n: u64) -> Duration {
    Duration::from_hours(n)
}

/// A store with two projects, one task in each.
fn fixture() -> (Store, [ProjectId; 2], [TaskId; 2]) {
    let mut store = Store::open_in_memory().unwrap();
    let t0 = Timestamp::from_millis(0);
    let work = store.create_project("Work", "#7c5cff", t0).unwrap();
    let home = store.create_project("Home", "#22c55e", t0).unwrap();
    let api = store.create_task(Some(work), "API", t0).unwrap();
    let garden = store.create_task(Some(home), "Garden", t0).unwrap();
    (store, [work, home], [api, garden])
}

fn ny(day: (i32, u32, u32), hour: u32, minute: u32) -> Timestamp {
    local(&New_York, day, (hour, minute))
}

fn day_totals(weekly: &Weekly) -> Vec<Duration> {
    weekly.days.iter().map(|day| day.total).collect()
}

fn ids(times: &[ProjectTime]) -> Vec<Option<ProjectId>> {
    times
        .iter()
        .map(|time| time.project.as_ref().map(|project| project.id))
        .collect()
}

#[test]
fn empty_week_has_seven_zero_days_from_monday() {
    let (store, _, _) = fixture();
    let weekly = weekly(&store, date(2026, 11, 2), &New_York).unwrap();
    assert_eq!(weekly.days.len(), 7);
    assert_eq!(weekly.days[0].date, date(2026, 11, 2));
    assert_eq!(weekly.days[0].date.weekday(), Weekday::Mon);
    assert_eq!(weekly.days[6].date, date(2026, 11, 8));
    assert_eq!(day_totals(&weekly), [Duration::ZERO; 7]);
    assert!(weekly.days.iter().all(|day| day.projects.is_empty()));
    assert_eq!(weekly.total, Duration::ZERO);
    assert_eq!(weekly.projects.len(), 0);
    assert_eq!(
        weekly.period,
        Period::week_of(date(2026, 11, 2), &New_York).unwrap()
    );
}

#[test]
fn any_day_of_the_week_gives_that_week() {
    let (store, _, _) = fixture();
    let from_sunday = weekly(&store, date(2026, 11, 8), &New_York).unwrap();
    let from_monday = weekly(&store, date(2026, 11, 2), &New_York).unwrap();
    assert_eq!(from_sunday, from_monday);
}

#[test]
fn days_split_projects_and_add_up_the_week() {
    let (mut store, [work, home], [api, garden]) = fixture();
    let wed = (2026, 11, 4);
    let thu = (2026, 11, 5);
    record(&mut store, Some(api), &[(ny(wed, 9, 0), ny(wed, 10, 0))]);
    record(
        &mut store,
        Some(garden),
        &[(ny(wed, 18, 0), ny(wed, 18, 30))],
    );
    record(
        &mut store,
        Some(garden),
        &[(ny(thu, 18, 0), ny(thu, 19, 0))],
    );
    record(&mut store, None, &[(ny(thu, 20, 0), ny(thu, 20, 10))]);

    let weekly = weekly(&store, date(2026, 11, 2), &New_York).unwrap();
    let z = Duration::ZERO;
    assert_eq!(day_totals(&weekly), [z, z, min(90), min(70), z, z, z]);
    assert_eq!(ids(&weekly.days[2].projects), [Some(work), Some(home)]);
    assert_eq!(weekly.days[2].projects[1].total, min(30));
    assert_eq!(ids(&weekly.days[3].projects), [Some(home), None]);
    assert_eq!(weekly.total, min(160));
    assert_eq!(ids(&weekly.projects), [Some(home), Some(work), None]);
    let totals: Vec<Duration> = weekly.projects.iter().map(|p| p.total).collect();
    assert_eq!(totals, [min(90), min(60), min(10)]);
    assert_eq!(
        weekly.projects[0].project.as_ref().unwrap().color,
        "#22c55e"
    );
}

#[test]
fn session_across_midnight_is_split_between_the_days() {
    let (mut store, [work, _], [api, _]) = fixture();
    let start = ny((2026, 11, 4), 23, 20);
    let end = ny((2026, 11, 5), 0, 50);
    record(&mut store, Some(api), &[(start, end)]);

    let weekly = weekly(&store, date(2026, 11, 4), &New_York).unwrap();
    assert_eq!(weekly.days[2].total, min(40));
    assert_eq!(weekly.days[3].total, min(50));
    assert_eq!(ids(&weekly.days[3].projects), [Some(work)]);
    assert_eq!(weekly.total, min(90));
}

#[test]
fn sunday_night_to_monday_belongs_to_two_weeks() {
    let tz = FixedOffset::west_opt(3 * 3600).unwrap();
    let (mut store, _, [api, _]) = fixture();
    let start = local(&tz, (2026, 11, 8), (23, 30));
    let end = local(&tz, (2026, 11, 9), (0, 30));
    record(&mut store, Some(api), &[(start, end)]);

    let first = weekly(&store, date(2026, 11, 2), &tz).unwrap();
    assert_eq!(first.days[6].total, min(30));
    assert_eq!(first.total, min(30));
    let second = weekly(&store, date(2026, 11, 9), &tz).unwrap();
    assert_eq!(second.days[0].total, min(30));
    assert_eq!(second.total, min(30));
}

#[test]
fn spring_forward_sunday_counts_23_hours() {
    let (mut store, _, [api, _]) = fixture();
    // From Saturday 23:00 to Monday 01:00 across the 2026-03-08 change in New York.
    let start = ny((2026, 3, 7), 23, 0);
    let end = ny((2026, 3, 9), 1, 0);
    record(&mut store, Some(api), &[(start, end)]);

    let weekly = weekly(&store, date(2026, 3, 2), &New_York).unwrap();
    assert_eq!(weekly.days[6].period.length(), hours(23));
    assert_eq!(weekly.days[5].total, hours(1));
    assert_eq!(weekly.days[6].total, hours(23));
    assert_eq!(weekly.total, hours(24));
}

#[test]
fn fall_back_sunday_counts_25_hours() {
    let (mut store, _, [api, _]) = fixture();
    // From Saturday 22:00 to Monday 02:00 across the 2026-11-01 change in New York.
    let start = ny((2026, 10, 31), 22, 0);
    let end = ny((2026, 11, 2), 2, 0);
    record(&mut store, Some(api), &[(start, end)]);
    // And a parallel session inside the repeated hour (01:00-02:00 happens twice).
    let repeated = ny((2026, 11, 1), 1, 0);
    let second_pass = Timestamp::from_millis(repeated.as_millis() + 3_600_000);
    record(
        &mut store,
        None,
        &[(second_pass, second_pass.plus(min(20)))],
    );

    let weekly = weekly(&store, date(2026, 10, 26), &New_York).unwrap();
    assert_eq!(weekly.days[6].period.length(), hours(25));
    assert_eq!(weekly.days[5].total, hours(2));
    assert_eq!(weekly.days[6].total, hours(25) + min(20));
    assert_eq!(weekly.total, hours(27) + min(20));
}
