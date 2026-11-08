use chrono_tz::America::New_York;

use super::*;
use crate::domain::TaskId;
use crate::reports::test_support::{date, local, min, record};
use crate::time::Timestamp;

/// 2026-11-04 `hour:minute` in New York.
fn wed(hour: u32, minute: u32) -> Timestamp {
    local(&New_York, (2026, 11, 4), (hour, minute))
}

fn wednesday() -> Period {
    Period::day(date(2026, 11, 4), &New_York).unwrap()
}

struct Fixture {
    store: Store,
    work: ProjectId,
    home: ProjectId,
    api: TaskId,
    docs: TaskId,
    garden: TaskId,
    loose: TaskId,
}

/// Two projects with tasks, plus a task without a project.
fn fixture() -> Fixture {
    let mut store = Store::open_in_memory().unwrap();
    let t0 = Timestamp::from_millis(0);
    let work = store.create_project("Work", "#7c5cff", t0).unwrap();
    let home = store.create_project("Home", "#22c55e", t0).unwrap();
    let api = store.create_task(Some(work), "API", t0).unwrap();
    let docs = store.create_task(Some(work), "Docs", t0).unwrap();
    let garden = store.create_task(Some(home), "Garden", t0).unwrap();
    let loose = store.create_task(None, "Loose", t0).unwrap();
    Fixture {
        store,
        work,
        home,
        api,
        docs,
        garden,
        loose,
    }
}

fn task_ids(group: &ProjectTotal) -> Vec<Option<TaskId>> {
    group
        .tasks
        .iter()
        .map(|row| row.task.as_ref().map(|task| task.id))
        .collect()
}

fn project_ids(overview: &Overview) -> Vec<Option<ProjectId>> {
    overview
        .projects
        .iter()
        .map(|group| group.project.as_ref().map(|project| project.id))
        .collect()
}

#[test]
fn empty_period_has_zero_total_and_no_rows() {
    let fx = fixture();
    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(overview.total, Duration::ZERO);
    assert_eq!(overview.projects.len(), 0);
    assert_eq!(overview.period, wednesday());
}

#[test]
fn totals_group_by_project_then_task_sorted_by_time() {
    let mut fx = fixture();
    record(&mut fx.store, Some(fx.api), &[(wed(9, 0), wed(9, 30))]);
    record(&mut fx.store, Some(fx.docs), &[(wed(10, 0), wed(10, 50))]);
    record(&mut fx.store, Some(fx.api), &[(wed(11, 0), wed(11, 10))]);
    record(&mut fx.store, Some(fx.garden), &[(wed(12, 0), wed(12, 20))]);

    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(overview.total, min(110));
    assert_eq!(project_ids(&overview), [Some(fx.work), Some(fx.home)]);
    let work = &overview.projects[0];
    assert_eq!(work.total, min(90));
    assert_eq!(task_ids(work), [Some(fx.docs), Some(fx.api)]);
    assert_eq!(work.tasks[0].total, min(50));
    assert_eq!(work.tasks[1].total, min(40));
    let info = work.project.as_ref().unwrap();
    assert_eq!(
        (info.name.as_str(), info.color.as_str()),
        ("Work", "#7c5cff")
    );
    assert!(!info.archived);
    let docs = work.tasks[0].task.as_ref().unwrap();
    assert_eq!(docs.name, "Docs");
    assert!(!docs.archived);
    assert_eq!(overview.projects[1].total, min(20));
}

#[test]
fn no_project_group_holds_loose_tasks_and_the_no_task_row() {
    let mut fx = fixture();
    record(&mut fx.store, Some(fx.loose), &[(wed(9, 0), wed(9, 15))]);
    record(&mut fx.store, None, &[(wed(10, 0), wed(10, 25))]);
    record(&mut fx.store, Some(fx.api), &[(wed(11, 0), wed(11, 5))]);

    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(overview.total, min(45));
    assert_eq!(project_ids(&overview), [None, Some(fx.work)]);
    let none = &overview.projects[0];
    assert_eq!(none.total, min(40));
    assert_eq!(task_ids(none), [None, Some(fx.loose)]);
    assert_eq!(none.tasks[0].total, min(25));
}

#[test]
fn session_across_midnight_is_split_between_the_days() {
    let mut fx = fixture();
    let thursday = |h, m| local(&New_York, (2026, 11, 5), (h, m));
    record(
        &mut fx.store,
        Some(fx.api),
        &[(wed(23, 20), thursday(0, 50))],
    );

    let wednesday_total = overview(&fx.store, wednesday()).unwrap().total;
    let thursday_period = Period::day(date(2026, 11, 5), &New_York).unwrap();
    let thursday_total = overview(&fx.store, thursday_period).unwrap().total;
    assert_eq!(wednesday_total, min(40));
    assert_eq!(thursday_total, min(50));
}

#[test]
fn custom_period_clips_the_segments() {
    let mut fx = fixture();
    record(
        &mut fx.store,
        Some(fx.api),
        &[(wed(9, 0), wed(9, 30)), (wed(10, 0), wed(10, 30))],
    );
    let period = Period::custom(wed(9, 20), wed(10, 10)).unwrap();
    let overview = overview(&fx.store, period).unwrap();
    assert_eq!(overview.total, min(20));
    assert_eq!(overview.projects[0].tasks[0].total, min(20));
}

#[test]
fn archived_project_still_shows_as_history() {
    let mut fx = fixture();
    record(&mut fx.store, Some(fx.garden), &[(wed(9, 0), wed(9, 30))]);
    fx.store.archive_project(fx.home, wed(20, 0)).unwrap();
    fx.store.archive_task(fx.garden, wed(20, 0)).unwrap();

    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(project_ids(&overview), [Some(fx.home)]);
    let info = overview.projects[0].project.as_ref().unwrap();
    assert!(info.archived);
    assert_eq!(info.color, "#22c55e");
    assert_eq!(task_ids(&overview.projects[0]), [Some(fx.garden)]);
    assert!(
        overview.projects[0].tasks[0]
            .task
            .as_ref()
            .unwrap()
            .archived
    );
}

#[test]
fn deleted_task_turns_its_sessions_into_no_task() {
    let mut fx = fixture();
    record(&mut fx.store, Some(fx.docs), &[(wed(9, 0), wed(9, 30))]);
    fx.store.delete_task(fx.docs).unwrap();

    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(project_ids(&overview), [None]);
    assert_eq!(task_ids(&overview.projects[0]), [None]);
    assert_eq!(overview.total, min(30));
}

#[test]
fn equal_totals_are_ordered_by_id_with_the_no_group_last() {
    let mut fx = fixture();
    record(&mut fx.store, None, &[(wed(8, 0), wed(8, 10))]);
    record(&mut fx.store, Some(fx.garden), &[(wed(9, 0), wed(9, 10))]);
    record(&mut fx.store, Some(fx.api), &[(wed(10, 0), wed(10, 10))]);
    record(&mut fx.store, Some(fx.docs), &[(wed(11, 0), wed(11, 10))]);

    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(project_ids(&overview), [Some(fx.work), Some(fx.home), None]);
    assert_eq!(
        task_ids(&overview.projects[0]),
        [Some(fx.api), Some(fx.docs)]
    );
}

#[test]
fn moved_task_is_grouped_under_its_current_project() {
    let mut fx = fixture();
    record(&mut fx.store, Some(fx.api), &[(wed(9, 0), wed(9, 30))]);
    fx.store.set_task_project(fx.api, Some(fx.home)).unwrap();

    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(project_ids(&overview), [Some(fx.home)]);
    assert_eq!(task_ids(&overview.projects[0]), [Some(fx.api)]);
    assert_eq!(overview.projects[0].total, min(30));
}

#[test]
fn deleted_project_moves_its_tasks_to_no_project() {
    let mut fx = fixture();
    record(&mut fx.store, Some(fx.api), &[(wed(9, 0), wed(9, 30))]);
    fx.store.delete_project(fx.work).unwrap();

    let overview = overview(&fx.store, wednesday()).unwrap();
    assert_eq!(project_ids(&overview), [None]);
    assert_eq!(task_ids(&overview.projects[0]), [Some(fx.api)]);
    assert_eq!(overview.total, min(30));
}
