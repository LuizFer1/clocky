use chrono_tz::America::New_York;

use super::*;
use crate::domain::{ProjectId, SubtaskId, TaskId, TimerId};
use crate::reports::test_support::{date, local, min, record, record_run};
use crate::store::SubtaskEventKind;
use crate::time::Timestamp;

/// 2026-11-04 `hour:minute` in New York.
fn wed(hour: u32, minute: u32) -> Timestamp {
    local(&New_York, (2026, 11, 4), (hour, minute))
}

fn seg(start: Timestamp, end: Timestamp) -> Segment {
    Segment { start, end }
}

/// A store with a project, a task in it and one subtask.
fn fixture() -> (Store, ProjectId, TaskId, SubtaskId) {
    let mut store = Store::open_in_memory().unwrap();
    let t0 = Timestamp::from_millis(0);
    let work = store.create_project("Work", "#7c5cff", t0).unwrap();
    let api = store.create_task(Some(work), "API", t0).unwrap();
    let outline = store.add_subtask(api, "Outline").unwrap();
    (store, work, api, outline)
}

#[test]
fn empty_day_has_zero_total_and_no_rows() {
    let (store, _, _, _) = fixture();
    let daily = daily(&store, date(2026, 11, 4), &New_York).unwrap();
    assert_eq!(daily.date, date(2026, 11, 4));
    assert_eq!(
        daily.period,
        Period::day(date(2026, 11, 4), &New_York).unwrap()
    );
    assert_eq!(daily.total, Duration::ZERO);
    assert_eq!(daily.blocks.len(), 0);
    assert_eq!(daily.markers.len(), 0);
}

#[test]
fn blocks_carry_the_session_and_its_segments_with_the_pauses_between() {
    let (mut store, work, api, _) = fixture();
    let first = record(
        &mut store,
        Some(api),
        &[(wed(9, 0), wed(9, 25)), (wed(9, 30), wed(9, 50))],
    );
    let focus = record_run(
        &mut store,
        None,
        (TimerId(5), Some(2)),
        TimerKind::Pomodoro,
        &[(wed(14, 0), wed(14, 25))],
    );

    let daily = daily(&store, date(2026, 11, 4), &New_York).unwrap();
    assert_eq!(daily.total, min(70));
    assert_eq!(daily.blocks.len(), 2);
    let block = &daily.blocks[0];
    assert_eq!(block.session, first);
    assert_eq!(block.kind, TimerKind::Stopwatch);
    assert_eq!(block.round, None);
    assert_eq!(block.task.as_ref().unwrap().id, api);
    let project = block.project.as_ref().unwrap();
    assert_eq!((project.id, project.color.as_str()), (work, "#7c5cff"));
    assert_eq!(
        block.segments,
        [seg(wed(9, 0), wed(9, 25)), seg(wed(9, 30), wed(9, 50))]
    );
    assert_eq!(block.total, min(45));
    let pomodoro = &daily.blocks[1];
    assert_eq!(pomodoro.session, focus);
    assert_eq!(pomodoro.kind, TimerKind::Pomodoro);
    assert_eq!(pomodoro.round, Some(2));
    assert_eq!(pomodoro.task, None);
    assert_eq!(pomodoro.project, None);
}

#[test]
fn session_across_midnight_is_clipped_to_each_day() {
    let (mut store, _, api, _) = fixture();
    let thu = |h, m| local(&New_York, (2026, 11, 5), (h, m));
    record(
        &mut store,
        Some(api),
        &[(wed(23, 0), wed(23, 30)), (wed(23, 40), thu(0, 50))],
    );

    let wednesday = daily(&store, date(2026, 11, 4), &New_York).unwrap();
    assert_eq!(
        wednesday.blocks[0].segments,
        [seg(wed(23, 0), wed(23, 30)), seg(wed(23, 40), thu(0, 0))]
    );
    assert_eq!(wednesday.total, min(50));
    let thursday = daily(&store, date(2026, 11, 5), &New_York).unwrap();
    assert_eq!(thursday.blocks[0].segments, [seg(thu(0, 0), thu(0, 50))]);
    assert_eq!(thursday.total, min(50));
}

#[test]
fn markers_include_events_with_and_without_a_session() {
    let (mut store, work, api, outline) = fixture();
    let tests = store.add_subtask(api, "Tests").unwrap();
    let run = TimerId(7);
    // During the session (linked), after it ended on the same run (a pomodoro break), and
    // with no timer at all; plus one the day before, which is left out.
    store
        .complete_subtask(outline, wed(9, 10), Some(run))
        .unwrap();
    store
        .complete_subtask(tests, wed(9, 40), Some(run))
        .unwrap();
    store.reopen_subtask(outline, wed(16, 0), None).unwrap();
    let tuesday = local(&New_York, (2026, 11, 3), (22, 0));
    store.reopen_subtask(tests, tuesday, None).unwrap();
    let session = record_run(
        &mut store,
        Some(api),
        (run, Some(1)),
        TimerKind::Pomodoro,
        &[(wed(9, 0), wed(9, 25))],
    );

    let daily = daily(&store, date(2026, 11, 4), &New_York).unwrap();
    let summary: Vec<_> = daily
        .markers
        .iter()
        .map(|m| (m.subtask, m.kind, m.at, m.session))
        .collect();
    assert_eq!(
        summary,
        [
            (
                outline,
                SubtaskEventKind::Completed,
                wed(9, 10),
                Some(session)
            ),
            (tests, SubtaskEventKind::Completed, wed(9, 40), None),
            (outline, SubtaskEventKind::Reopened, wed(16, 0), None),
        ]
    );
    let marker = &daily.markers[0];
    assert_eq!(marker.name, "Outline");
    assert_eq!((marker.task.id, marker.task.name.as_str()), (api, "API"));
    assert!(!marker.task.archived);
    assert_eq!(marker.project.as_ref().unwrap().id, work);
}

#[test]
fn marker_may_point_to_a_session_with_no_block_that_day() {
    let (mut store, _, api, outline) = fixture();
    let run = TimerId(8);
    // Checked on Wednesday, during a pause that lasts from Tuesday night to Thursday.
    store
        .complete_subtask(outline, wed(12, 0), Some(run))
        .unwrap();
    let tuesday = |h, m| local(&New_York, (2026, 11, 3), (h, m));
    let thursday = |h, m| local(&New_York, (2026, 11, 5), (h, m));
    let session = record_run(
        &mut store,
        Some(api),
        (run, None),
        TimerKind::Stopwatch,
        &[
            (tuesday(23, 0), tuesday(23, 30)),
            (thursday(0, 10), thursday(0, 20)),
        ],
    );

    let daily = daily(&store, date(2026, 11, 4), &New_York).unwrap();
    assert_eq!(daily.blocks.len(), 0);
    assert_eq!(daily.total, Duration::ZERO);
    assert_eq!(daily.markers.len(), 1);
    assert_eq!(daily.markers[0].session, Some(session));
}
