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
