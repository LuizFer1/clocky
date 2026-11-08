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
fn corrupted_active_timer_gives_the_store_back_for_recovery() {
    let db = TempDb::new("reopen-corrupt");
    let (mut tracker, _) = open(&db, at(0));
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let sw = tracker.start_stopwatch(Some(task), at(0)).unwrap();
    let _events = tracker.stop(sw, at(60)).unwrap();
    tracker
        .start_pomodoro(None, quick(false), None, at(100))
        .unwrap();
    tracker.store().execute_batch_for_tests(
        "PRAGMA ignore_check_constraints = ON; UPDATE active_timers SET phase = 'nap';",
    );
    drop(tracker);

    let failed = Tracker::open(Store::open(db.path()).unwrap(), at(200)).unwrap_err();
    assert!(matches!(
        failed.error,
        TrackerError::Store(StoreError::Corrupt { .. })
    ));
    assert!(failed.to_string().starts_with("invalid pomodoro phase"));
    assert!(std::error::Error::source(&failed).is_some());
    // History is still there, and discarding the active timers lets the tracker open.
    assert_eq!(failed.store.task_time(task).unwrap(), secs(60));
    let store = failed.discard_active_timers().unwrap();
    let (tracker, events) = Tracker::open(store, at(200)).unwrap();
    assert_eq!(events, vec![]);
    assert_eq!(tracker.timers().iter().count(), 0);
}

#[test]
fn failed_catch_up_on_open_gives_the_store_back() {
    let db = TempDb::new("reopen-failed-write");
    let (mut tracker, _) = open(&db, at(0));
    tracker
        .start_countdown(None, secs(60), None, at(0))
        .unwrap();
    drop(tracker);

    let store = Store::open(db.path()).unwrap();
    store.execute_batch_for_tests(
        "CREATE TEMP TRIGGER no_sessions BEFORE INSERT ON sessions
         BEGIN SELECT RAISE(ABORT, 'disk is broken'); END;",
    );
    let failed = Tracker::open(store, at(500)).unwrap_err();
    assert!(matches!(
        failed.error,
        TrackerError::Store(StoreError::Sqlite(_))
    ));
    assert_eq!(failed.store.load_active().unwrap().1.len(), 1);
}

fn classic(auto_start: bool) -> PomodoroConfig {
    PomodoroConfig::new(mins(25), mins(5), mins(15), 4, auto_start).unwrap()
}

fn pomodoro(tracker: &Tracker, id: TimerId) -> Pomodoro {
    let Timer::Pomodoro(p) = &tracker.timers().get(id).unwrap().timer else {
        panic!("expected a pomodoro");
    };
    p.clone()
}

#[test]
fn reopening_within_the_grace_still_stops_the_pomodoro() {
    let db = TempDb::new("reopen-grace");
    let (mut tracker, _) = open(&db, at(0));
    let pomo = tracker
        .start_pomodoro(None, classic(true), None, at(0))
        .unwrap();
    drop(tracker);

    let (tracker, events) = open(&db, at(25 * 60 + 30));
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(session.session.ended_at, at(25 * 60));
    let p = pomodoro(&tracker, pomo);
    assert_eq!(p.phase(), Phase::ShortBreak);
    assert!(p.is_waiting());
}

#[test]
fn reopening_after_a_long_break_does_not_start_the_next_round() {
    let db = TempDb::new("reopen-long-break");
    let (mut tracker, _) = open(&db, at(0));
    let pomo = tracker
        .start_pomodoro(None, quick(false), None, at(0))
        .unwrap();
    let _events = tracker.skip(pomo, at(1)).unwrap(); // short break waits
    let _events = tracker.skip(pomo, at(2)).unwrap(); // focus 2 waits
    let _events = tracker.skip(pomo, at(3)).unwrap(); // long break waits
    tracker.start_phase(pomo, at(3)).unwrap(); // due at 13
    drop(tracker);

    let (tracker, events) = open(&db, at(13 + 30));
    assert_eq!(recorded(&events).len(), 0);
    let p = pomodoro(&tracker, pomo);
    assert_eq!(
        (p.phase(), p.round(), p.focus_index()),
        (Phase::Focus, 2, 1)
    );
    assert!(p.is_waiting());
}

#[test]
fn reopening_never_records_phases_that_did_not_happen() {
    let db = TempDb::new("reopen-short-phases");
    let (mut tracker, _) = open(&db, at(0));
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let pomo = tracker
        .start_pomodoro(Some(task), quick(true), None, at(0))
        .unwrap();
    drop(tracker);

    let (tracker, events) = open(&db, at(40));
    assert_eq!(recorded(&events).len(), 1);
    let sessions = tracker.store().recent_sessions(task, 10).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].ended_at, at(10));
    let p = pomodoro(&tracker, pomo);
    assert_eq!((p.phase(), p.focus_index()), (Phase::ShortBreak, 1));
    assert!(p.is_waiting());
}
