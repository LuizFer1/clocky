use super::*;
use crate::domain::SubtaskId;
use crate::test_util::{at, completed, secs};

/// A store with one task and one subtask in it.
fn fixture() -> (Store, TaskId, SubtaskId) {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    let subtask = store.add_subtask(task, "outline").unwrap();
    (store, task, subtask)
}

fn count(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn record_and_read_back() {
    let (mut store, task, _) = fixture();
    let stopwatch = completed(TimerKind::Stopwatch, &[(0, 10), (20, 30)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &stopwatch)
        .unwrap();
    let detail = store.session(id).unwrap();
    assert_eq!(
        detail.session,
        Session {
            id,
            kind: TimerKind::Stopwatch,
            task: Some(task),
            run: TimerId(1),
            round: None,
            started_at: at(0),
            ended_at: at(30),
            effective: secs(20),
        }
    );
    assert_eq!(detail.segments, stopwatch.segments());
    assert_eq!(detail.events.len(), 0);
}

#[test]
fn pomodoro_round_is_recorded() {
    let (mut store, _, _) = fixture();
    let focus = completed(TimerKind::Pomodoro, &[(0, 1500)]);
    let id = store
        .record_session(None, TimerId(7), Some(2), &focus)
        .unwrap();
    let session = store.session(id).unwrap().session;
    assert_eq!(session.kind, TimerKind::Pomodoro);
    assert_eq!(session.round, Some(2));
    assert_eq!(session.task, None);
}

#[test]
fn zero_length_segments_are_dropped() {
    let (mut store, _, _) = fixture();
    let countdown = completed(TimerKind::Countdown, &[(0, 10), (15, 15), (20, 30)]);
    let id = store
        .record_session(None, TimerId(1), None, &countdown)
        .unwrap();
    let detail = store.session(id).unwrap();
    assert_eq!(detail.segments.len(), 2);
    assert_eq!(detail.session.effective, secs(20));
}

#[test]
fn event_inside_the_window_is_linked() {
    let (mut store, task, subtask) = fixture();
    store
        .complete_subtask(subtask, at(15), Some(TimerId(1)))
        .unwrap();
    let stopwatch = completed(TimerKind::Stopwatch, &[(0, 10), (20, 30)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &stopwatch)
        .unwrap();
    let events = store.session(id).unwrap().events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session, Some(id));
    assert_eq!(store.subtask_events(subtask).unwrap()[0].session, Some(id));
}

#[test]
fn event_in_a_pomodoro_break_stays_unlinked() {
    let (mut store, task, subtask) = fixture();
    let run = TimerId(1);
    let first = completed(TimerKind::Pomodoro, &[(0, 1500)]);
    store
        .record_session(Some(task), run, Some(1), &first)
        .unwrap();
    store
        .complete_subtask(subtask, at(1620), Some(run))
        .unwrap();
    let second = completed(TimerKind::Pomodoro, &[(1800, 3300)]);
    let id = store
        .record_session(Some(task), run, Some(1), &second)
        .unwrap();
    assert_eq!(store.session(id).unwrap().events.len(), 0);
    let events = store.subtask_events(subtask).unwrap();
    assert_eq!(events[0].run, Some(run));
    assert_eq!(events[0].session, None);
}

#[test]
fn two_timers_on_the_same_task_keep_their_own_events() {
    let (mut store, task, subtask) = fixture();
    store
        .complete_subtask(subtask, at(10), Some(TimerId(1)))
        .unwrap();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    let other = store
        .record_session(Some(task), TimerId(2), None, &span)
        .unwrap();
    assert_eq!(store.session(other).unwrap().events.len(), 0);
    let own = store
        .record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
    assert_eq!(store.session(own).unwrap().events.len(), 1);
}

#[test]
fn events_follow_the_timer_when_its_task_changes() {
    let (mut store, first_task, first_subtask) = fixture();
    let second_task = store.create_task(None, "review", at(0)).unwrap();
    let second_subtask = store.add_subtask(second_task, "read").unwrap();
    let run = TimerId(1);
    store
        .complete_subtask(first_subtask, at(10), Some(run))
        .unwrap();
    store
        .complete_subtask(second_subtask, at(20), Some(run))
        .unwrap();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    let id = store
        .record_session(Some(second_task), run, None, &span)
        .unwrap();
    let detail = store.session(id).unwrap();
    assert_eq!(detail.session.task, Some(second_task));
    let subtasks: Vec<_> = detail.events.iter().map(|event| event.subtask).collect();
    assert_eq!(subtasks, vec![first_subtask, second_subtask]);
    assert_ne!(first_task, second_task);
}

#[test]
fn events_outside_a_timer_are_never_linked() {
    let (mut store, task, subtask) = fixture();
    store.complete_subtask(subtask, at(10), None).unwrap();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
    assert_eq!(store.session(id).unwrap().events.len(), 0);
}

#[test]
fn failed_record_writes_nothing() {
    let (mut store, _, _) = fixture();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    assert!(matches!(
        store.record_session(Some(TaskId(99)), TimerId(1), None, &span),
        Err(StoreError::Sqlite(_))
    ));
    assert!(matches!(
        store.record_session(None, TimerId(u64::MAX), None, &span),
        Err(StoreError::OutOfRange("timer id"))
    ));
    assert_eq!(count(&store, "sessions"), 0);
    assert_eq!(count(&store, "session_segments"), 0);
}

#[test]
fn missing_session_is_not_found() {
    let (store, _, _) = fixture();
    assert!(matches!(
        store.session(SessionId(5)),
        Err(StoreError::NotFound {
            entity: "session",
            id: 5
        })
    ));
}

#[test]
fn corrupted_sessions_are_errors() {
    let (mut store, _, _) = fixture();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    let id = store.record_session(None, TimerId(1), None, &span).unwrap();
    let corrupt = |store: &Store, sql: &str| {
        store
            .conn
            .execute_batch(&format!("PRAGMA ignore_check_constraints = ON; {sql}"))
            .unwrap();
        store.session(id)
    };
    let result = corrupt(&store, "UPDATE sessions SET effective_ms = -1;");
    assert!(matches!(
        result,
        Err(StoreError::OutOfRange("session duration"))
    ));
    let result = corrupt(
        &store,
        "UPDATE sessions SET effective_ms = 1, kind = 'bogus';",
    );
    assert!(matches!(result, Err(StoreError::Corrupt { .. })));
    let result = corrupt(
        &store,
        "UPDATE sessions SET kind = 'stopwatch', run_id = -1;",
    );
    assert!(matches!(result, Err(StoreError::OutOfRange("timer id"))));
    let result = corrupt(
        &store,
        "UPDATE sessions SET run_id = 1, round = 5000000000;",
    );
    assert!(matches!(
        result,
        Err(StoreError::OutOfRange("session round"))
    ));
    let result = corrupt(
        &store,
        "UPDATE sessions SET round = NULL; UPDATE session_segments SET end_ms = 0;",
    );
    assert!(matches!(result, Err(StoreError::Corrupt { .. })));
}

#[test]
fn deleting_a_session_keeps_its_events_without_session() {
    let (mut store, task, subtask) = fixture();
    store
        .complete_subtask(subtask, at(10), Some(TimerId(1)))
        .unwrap();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
    store.delete_session(id).unwrap();
    assert!(store.session(id).is_err());
    assert_eq!(count(&store, "session_segments"), 0);
    let events = store.subtask_events(subtask).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session, None);
}

#[test]
fn changing_the_task_of_a_session_changes_only_the_task() {
    let (mut store, task, subtask) = fixture();
    let other = store.create_task(None, "review", at(0)).unwrap();
    store
        .complete_subtask(subtask, at(10), Some(TimerId(1)))
        .unwrap();
    let span = completed(TimerKind::Stopwatch, &[(0, 10), (20, 30)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
    let before = store.session(id).unwrap();
    store.set_session_task(id, Some(other)).unwrap();
    let after = store.session(id).unwrap();
    assert_eq!(after.session.task, Some(other));
    let mut expected = before;
    expected.session.task = Some(other);
    assert_eq!(after, expected);
    store.set_session_task(id, None).unwrap();
    assert_eq!(store.session(id).unwrap().session.task, None);
}

#[test]
fn deleting_a_task_keeps_its_sessions_without_task() {
    let (mut store, task, _) = fixture();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
    store.delete_task(task).unwrap();
    assert_eq!(store.session(id).unwrap().session.task, None);
}

#[test]
fn recent_sessions_are_newest_first_and_limited() {
    let (mut store, task, _) = fixture();
    let mut ids = Vec::new();
    for start in [0, 100, 200] {
        let span = completed(TimerKind::Stopwatch, &[(start, start + 30)]);
        ids.push(
            store
                .record_session(Some(task), TimerId(1), None, &span)
                .unwrap(),
        );
    }
    let other = completed(TimerKind::Stopwatch, &[(300, 330)]);
    store
        .record_session(None, TimerId(2), None, &other)
        .unwrap();
    let recent: Vec<_> = store
        .recent_sessions(task, 2)
        .unwrap()
        .iter()
        .map(|session| session.id)
        .collect();
    assert_eq!(recent, vec![ids[2], ids[1]]);
}

#[test]
fn editing_a_missing_session_is_not_found() {
    let (mut store, task, _) = fixture();
    assert!(matches!(
        store.delete_session(SessionId(5)),
        Err(StoreError::NotFound { id: 5, .. })
    ));
    assert!(store.set_session_task(SessionId(5), Some(task)).is_err());
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    let id = store.record_session(None, TimerId(1), None, &span).unwrap();
    assert!(matches!(
        store.set_session_task(id, Some(TaskId(99))),
        Err(StoreError::Sqlite(_))
    ));
}

#[test]
fn bounds_come_from_the_segments_that_are_kept() {
    let (mut store, task, subtask) = fixture();
    store
        .complete_subtask(subtask, at(25), Some(TimerId(1)))
        .unwrap();
    let edges = completed(TimerKind::Stopwatch, &[(0, 0), (10, 20), (30, 30)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &edges)
        .unwrap();
    let detail = store.session(id).unwrap();
    assert_eq!(detail.session.started_at, at(10));
    assert_eq!(detail.session.ended_at, at(20));
    assert_eq!(detail.session.effective, secs(10));
    assert_eq!(detail.events.len(), 0);
}

#[test]
fn a_failing_segment_insert_rolls_back_the_whole_session() {
    let (mut store, task, subtask) = fixture();
    store
        .complete_subtask(subtask, at(10), Some(TimerId(1)))
        .unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER fail_segments BEFORE INSERT ON session_segments
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let span = completed(TimerKind::Stopwatch, &[(0, 30)]);
    assert!(matches!(
        store.record_session(Some(task), TimerId(1), None, &span),
        Err(StoreError::Sqlite(_))
    ));
    assert_eq!(count(&store, "sessions"), 0);
    assert_eq!(store.subtask_events(subtask).unwrap()[0].session, None);
}

#[test]
fn linking_window_includes_both_edges_only() {
    let (mut store, task, first) = fixture();
    let second = store.add_subtask(task, "draft").unwrap();
    let before = store.add_subtask(task, "before").unwrap();
    let after = store.add_subtask(task, "after").unwrap();
    let run = Some(TimerId(1));
    store.complete_subtask(first, at(10), run).unwrap();
    store.complete_subtask(second, at(20), run).unwrap();
    store
        .complete_subtask(before, Timestamp::from_millis(9_999), run)
        .unwrap();
    store
        .complete_subtask(after, Timestamp::from_millis(20_001), run)
        .unwrap();
    let span = completed(TimerKind::Stopwatch, &[(10, 20)]);
    let id = store
        .record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
    let linked: Vec<_> = store
        .session(id)
        .unwrap()
        .events
        .iter()
        .map(|event| event.subtask)
        .collect();
    assert_eq!(linked, vec![first, second]);
}
