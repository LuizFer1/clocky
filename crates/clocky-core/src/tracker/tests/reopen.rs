//! Closing and reopening the app: every scenario drops the tracker and opens the same file.

use super::*;
use crate::engine::timers::ActiveTimer;
use crate::test_util::TempDb;

fn open(db: &TempDb, now: Timestamp) -> (Tracker, Vec<TrackerEvent>) {
    Tracker::open(Store::open(db.path()).unwrap(), now).unwrap()
}

fn saved(tracker: &Tracker) -> Vec<ActiveTimer> {
    tracker.timers().iter().cloned().collect()
}

#[test]
fn empty_database_opens_without_timers() {
    let db = TempDb::new("reopen-empty");
    let (tracker, events) = open(&db, at(0));
    assert_eq!(events, vec![]);
    assert_eq!(tracker.timers().iter().count(), 0);
}

#[test]
fn running_paused_and_waiting_timers_survive_a_restart() {
    let db = TempDb::new("reopen-roundtrip");
    let (mut tracker, _) = open(&db, at(0));
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let sw = tracker.start_stopwatch(Some(task), at(0)).unwrap();
    tracker.pause(sw, at(10)).unwrap();
    tracker.resume(sw, at(20)).unwrap();
    let cd = tracker
        .start_countdown(None, mins(10), None, at(0))
        .unwrap();
    tracker.pause(cd, at(60)).unwrap();
    let pomo = tracker
        .start_pomodoro(Some(task), quick(false), None, at(0))
        .unwrap();
    let _events = tracker.tick(at(10)).unwrap(); // focus done: the short break waits
    let before = saved(&tracker);
    drop(tracker);

    let (mut tracker, events) = open(&db, at(100));
    assert_eq!(events, vec![]);
    assert_eq!(saved(&tracker), before);
    // They keep working: the stopwatch counts the closed time, nothing was lost.
    tracker.start_phase(pomo, at(100)).unwrap();
    tracker.resume(cd, at(100)).unwrap();
    let events = tracker.stop(sw, at(200)).unwrap();
    let detail = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(detail.session.effective, secs(190));
    assert_eq!(detail.segments.len(), 2);
}

#[test]
fn countdown_due_while_closed_is_recorded_at_its_due_time() {
    let db = TempDb::new("reopen-countdown");
    let (mut tracker, _) = open(&db, at(0));
    let cd = tracker
        .start_countdown(None, secs(60), None, at(0))
        .unwrap();
    drop(tracker);

    let (tracker, events) = open(&db, at(5000));
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[0],
        TrackerEvent::Timer(TimersEvent::CountdownFinished {
            timer: cd,
            at: at(60)
        })
    );
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(session.session.ended_at, at(60));
    assert_eq!(session.session.effective, secs(60));
    drop(tracker);

    let (tracker, events) = open(&db, at(6000));
    assert_eq!(events, vec![]);
    assert_eq!(tracker.timers().iter().count(), 0);
}

#[test]
fn pomodoro_due_while_closed_closes_at_its_due_time_and_waits() {
    let db = TempDb::new("reopen-pomodoro");
    let (mut tracker, _) = open(&db, at(0));
    let pomo = tracker
        .start_pomodoro(None, quick(true), None, at(0))
        .unwrap();
    drop(tracker);

    let (tracker, events) = open(&db, at(3 * 3600));
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(
        (session.session.started_at, session.session.ended_at),
        (at(0), at(10))
    );
    assert_eq!(
        events.last(),
        Some(&TrackerEvent::Timer(TimersEvent::PomodoroPhaseEnded {
            timer: pomo,
            ended: Phase::Focus,
            next: Phase::ShortBreak,
            auto_started: false,
        }))
    );
    let waiting = saved(&tracker);
    drop(tracker);

    let (tracker, events) = open(&db, at(4 * 3600));
    assert_eq!(events, vec![]);
    assert_eq!(saved(&tracker), waiting);
}

#[test]
fn forgotten_stopwatch_is_reported_after_reopening() {
    let db = TempDb::new("reopen-forgotten");
    let (mut tracker, _) = open(&db, at(0));
    let kept = tracker.start_stopwatch(None, at(0)).unwrap();
    let dropped = tracker.start_stopwatch(None, at(0)).unwrap();
    drop(tracker);

    let nine_hours = at(9 * 3600);
    let (mut tracker, events) = open(&db, nine_hours);
    assert_eq!(events, vec![]);
    assert_eq!(tracker.forgotten(nine_hours).unwrap(), vec![kept, dropped]);
    let events = tracker.stop(kept, nine_hours).unwrap();
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(session.session.effective, secs(9 * 3600));
    tracker.discard(dropped).unwrap();
    drop(tracker);

    let (tracker, _) = open(&db, nine_hours);
    assert_eq!(tracker.timers().iter().count(), 0);
    assert_eq!(tracker.store().recent_tasks(10).unwrap().len(), 0);
}

#[test]
fn timer_ids_are_never_reused() {
    let db = TempDb::new("reopen-ids");
    let (mut tracker, _) = open(&db, at(0));
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let first = tracker
        .start_pomodoro(Some(task), quick(false), None, at(0))
        .unwrap();
    let _events = tracker.tick(at(10)).unwrap(); // focus 1 recorded
    let _events = tracker.stop(first, at(11)).unwrap();
    assert_eq!(tracker.timers().iter().count(), 0);
    drop(tracker);

    let (mut tracker, _) = open(&db, at(100));
    let second = tracker
        .start_pomodoro(Some(task), quick(false), None, at(100))
        .unwrap();
    assert!(second > first);
    let _events = tracker.tick(at(110)).unwrap();
    let sessions = tracker.store().recent_sessions(task, 10).unwrap();
    let runs: Vec<TimerId> = sessions.iter().map(|s| s.run).collect();
    assert_eq!(runs, vec![second, first]);
}

#[test]
fn corrupted_active_timer_is_an_error_on_open() {
    let db = TempDb::new("reopen-corrupt");
    let (mut tracker, _) = open(&db, at(0));
    tracker
        .start_pomodoro(None, quick(false), None, at(0))
        .unwrap();
    tracker.store().execute_batch_for_tests(
        "PRAGMA ignore_check_constraints = ON; UPDATE active_timers SET phase = 'nap';",
    );
    drop(tracker);

    let result = Tracker::open(Store::open(db.path()).unwrap(), at(1));
    assert!(matches!(
        result,
        Err(TrackerError::Store(StoreError::Corrupt { .. }))
    ));
}
