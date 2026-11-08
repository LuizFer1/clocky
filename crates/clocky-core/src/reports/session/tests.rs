use chrono_tz::America::New_York;

use super::*;
use crate::domain::{ProjectId, SubtaskId, TaskId, TimerId};
use crate::reports::test_support::{local, min, record, record_run};
use crate::store::{StoreError, SubtaskEventKind};

/// 2026-11-04 `hour:minute` in New York.
fn wed(hour: u32, minute: u32) -> Timestamp {
    local(&New_York, (2026, 11, 4), (hour, minute))
}

fn seg(start: Timestamp, end: Timestamp) -> Segment {
    Segment { start, end }
}

const RUN: TimerId = TimerId(42);

/// A store with a project, a task in it and two subtasks.
fn fixture() -> (Store, ProjectId, TaskId, [SubtaskId; 2]) {
    let mut store = Store::open_in_memory().unwrap();
    let t0 = Timestamp::from_millis(0);
    let work = store.create_project("Work", "#7c5cff", t0).unwrap();
    let api = store.create_task(Some(work), "API", t0).unwrap();
    let outline = store.add_subtask(api, "Outline").unwrap();
    let tests = store.add_subtask(api, "Tests").unwrap();
    (store, work, api, [outline, tests])
}

/// A pomodoro focus of round 3 with two pauses: 9:00-9:10, 9:15-9:23 and 9:30-9:40.
fn paused_focus(store: &mut Store, task: TaskId) -> SessionId {
    record_run(
        store,
        Some(task),
        (RUN, Some(3)),
        TimerKind::Pomodoro,
        &[
            (wed(9, 0), wed(9, 10)),
            (wed(9, 15), wed(9, 23)),
            (wed(9, 30), wed(9, 40)),
        ],
    )
}

#[test]
fn unknown_session_is_not_found() {
    let (store, _, _, _) = fixture();
    assert!(matches!(
        session_report(&store, SessionId(99)),
        Err(ReportError::Store(StoreError::NotFound {
            entity: "session",
            id: 99
        }))
    ));
}

#[test]
fn session_without_pauses_has_equal_effective_and_wall_time() {
    let (mut store, work, api, _) = fixture();
    let id = record(&mut store, Some(api), &[(wed(9, 0), wed(9, 25))]);

    let report = session_report(&store, id).unwrap();
    assert_eq!(report.session, id);
    assert_eq!(report.kind, TimerKind::Stopwatch);
    assert_eq!(report.round, None);
    assert_eq!(
        (report.started_at, report.ended_at),
        (wed(9, 0), wed(9, 25))
    );
    assert_eq!(report.effective, min(25));
    assert_eq!(report.wall, min(25));
    assert_eq!(report.segments, [seg(wed(9, 0), wed(9, 25))]);
    assert_eq!(report.pauses.len(), 0);
    assert_eq!(report.milestones.len(), 0);
    let task = report.task.as_ref().unwrap();
    assert_eq!((task.id, task.name.as_str()), (api, "API"));
    let project = report.project.as_ref().unwrap();
    assert_eq!(
        (project.id, project.name.as_str(), project.color.as_str()),
        (work, "Work", "#7c5cff")
    );
}

#[test]
fn pauses_are_the_gaps_between_segments() {
    let (mut store, _, api, _) = fixture();
    let id = paused_focus(&mut store, api);

    let report = session_report(&store, id).unwrap();
    assert_eq!(report.kind, TimerKind::Pomodoro);
    assert_eq!(report.round, Some(3));
    assert_eq!(report.effective, min(28));
    assert_eq!(report.wall, min(40));
    assert_eq!(
        report.pauses,
        [
            Pause {
                start: wed(9, 10),
                end: wed(9, 15),
                duration: min(5),
            },
            Pause {
                start: wed(9, 23),
                end: wed(9, 30),
                duration: min(7),
            },
        ]
    );
}

#[test]
fn milestones_count_running_time_from_the_start() {
    let (mut store, _, api, [outline, tests]) = fixture();
    // 5 min in; at the end of the second segment (18 min of focus after a 5 min pause);
    // during the second pause (the clock is stopped, still 18 min); 3 min into the last one.
    store
        .complete_subtask(outline, wed(9, 5), Some(RUN))
        .unwrap();
    store
        .complete_subtask(tests, wed(9, 23), Some(RUN))
        .unwrap();
    store
        .reopen_subtask(outline, wed(9, 26), Some(RUN))
        .unwrap();
    store.reopen_subtask(tests, wed(9, 33), Some(RUN)).unwrap();
    let id = paused_focus(&mut store, api);

    let report = session_report(&store, id).unwrap();
    let summary: Vec<_> = report
        .milestones
        .iter()
        .map(|m| (m.marker.subtask, m.marker.kind, m.marker.at, m.offset))
        .collect();
    assert_eq!(
        summary,
        [
            (outline, SubtaskEventKind::Completed, wed(9, 5), min(5)),
            (tests, SubtaskEventKind::Completed, wed(9, 23), min(18)),
            (outline, SubtaskEventKind::Reopened, wed(9, 26), min(18)),
            (tests, SubtaskEventKind::Reopened, wed(9, 33), min(21)),
        ]
    );
    assert_eq!(report.milestones[1].marker.name, "Tests");
    assert_eq!(report.milestones[1].marker.session, Some(id));
}

#[test]
fn events_outside_the_session_are_not_milestones() {
    let (mut store, _, api, [outline, tests]) = fixture();
    // During the break after the focus (same run), and with no timer during the focus.
    store
        .complete_subtask(outline, wed(9, 45), Some(RUN))
        .unwrap();
    store.complete_subtask(tests, wed(9, 5), None).unwrap();
    let id = paused_focus(&mut store, api);

    let report = session_report(&store, id).unwrap();
    assert_eq!(report.milestones.len(), 0);
}

#[test]
fn deleted_task_leaves_no_task_and_no_project() {
    let (mut store, _, api, _) = fixture();
    let id = record(&mut store, Some(api), &[(wed(9, 0), wed(9, 25))]);
    store.delete_task(api).unwrap();

    let report = session_report(&store, id).unwrap();
    assert_eq!((report.task, report.project), (None, None));
}

#[test]
fn task_without_a_project_has_no_project() {
    let (mut store, _, _, _) = fixture();
    let loose = store
        .create_task(None, "Loose", Timestamp::from_millis(0))
        .unwrap();
    let id = record(&mut store, Some(loose), &[(wed(9, 0), wed(9, 25))]);

    let report = session_report(&store, id).unwrap();
    assert_eq!(report.task.unwrap().name, "Loose");
    assert_eq!(report.project, None);
}

#[test]
fn touching_segments_make_no_pause() {
    let segments = [seg(wed(9, 0), wed(9, 10)), seg(wed(9, 10), wed(9, 20))];
    assert_eq!(pauses(&segments), []);
    assert_eq!(running_time_until(&segments, wed(9, 15)), min(15));
    assert_eq!(running_time_until(&segments, wed(8, 0)), Duration::ZERO);
    assert_eq!(running_time_until(&segments, wed(10, 0)), min(20));
}

#[test]
fn archived_task_and_project_are_flagged() {
    let (mut store, work, api, _) = fixture();
    let id = record(&mut store, Some(api), &[(wed(9, 0), wed(9, 25))]);
    store.archive_task(api, wed(20, 0)).unwrap();
    store.archive_project(work, wed(20, 0)).unwrap();

    let report = session_report(&store, id).unwrap();
    assert!(report.task.unwrap().archived);
    assert!(report.project.unwrap().archived);
}
