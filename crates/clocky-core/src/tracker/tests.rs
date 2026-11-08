use super::*;
use crate::engine::pomodoro::Phase;
use crate::engine::session::TimerKind;
use crate::store::StoreError;
use crate::test_util::{at, mins, secs};

mod reopen;

fn tracker() -> Tracker {
    let (tracker, events) = Tracker::open(Store::open_in_memory().unwrap(), at(0)).unwrap();
    assert_eq!(events, vec![]);
    tracker
}

/// 10s focus, 10s breaks, 2 focus phases per round.
fn quick(auto_start: bool) -> PomodoroConfig {
    PomodoroConfig::new(secs(10), secs(10), secs(10), 2, auto_start).unwrap()
}

/// The ids of the sessions recorded in `events`.
fn recorded(events: &[TrackerEvent]) -> Vec<SessionId> {
    events
        .iter()
        .filter_map(|event| match event {
            TrackerEvent::SessionRecorded { session, .. } => Some(*session),
            TrackerEvent::Timer(_) => None,
        })
        .collect()
}

/// Makes every write to sessions and active timers fail, as a broken disk would.
fn break_writes(tracker: &Tracker) {
    for table in ["sessions", "active_timers"] {
        for op in ["INSERT", "UPDATE", "DELETE"] {
            tracker.store().execute_batch_for_tests(&format!(
                "CREATE TRIGGER fail_{op}_{table} BEFORE {op} ON {table}
                 BEGIN SELECT RAISE(ABORT, 'disk is broken'); END;"
            ));
        }
    }
}

#[test]
fn stop_records_the_session_and_forgets_the_timer() {
    let mut tracker = tracker();
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let id = tracker.start_stopwatch(Some(task), at(0)).unwrap();
    tracker.pause(id, at(10)).unwrap();
    tracker.resume(id, at(20)).unwrap();
    let events = tracker.stop(id, at(30)).unwrap();
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        TrackerEvent::Timer(TimersEvent::SessionCompleted { timer, .. }) if *timer == id
    ));
    let session = recorded(&events)[0];
    let detail = tracker.store().session(session).unwrap();
    assert_eq!(detail.session.task, Some(task));
    assert_eq!(detail.session.run, id);
    assert_eq!(detail.session.effective, secs(20));
    assert_eq!(detail.segments.len(), 2);
    assert!(tracker.timers().get(id).is_none());
    assert_eq!(tracker.store().load_active().unwrap().1, vec![]);
}

#[test]
fn every_command_is_saved() {
    let mut tracker = tracker();
    let preset = tracker
        .store_mut()
        .create_timer_preset("tea", mins(5), None)
        .unwrap();
    let cd = tracker
        .start_countdown(None, mins(5), Some(preset), at(0))
        .unwrap();
    let pomo = tracker
        .start_pomodoro(None, quick(false), None, at(0))
        .unwrap();
    tracker.pause(cd, at(30)).unwrap();
    let _events = tracker.skip(pomo, at(5)).unwrap(); // short break waits
    tracker.start_phase(pomo, at(6)).unwrap();
    let saved: Vec<_> = tracker.timers().iter().cloned().collect();
    assert_eq!(tracker.store().load_active().unwrap().1, saved);
    assert_eq!(saved[0].preset, Some(PresetRef::Timer(preset)));
}

#[test]
fn skip_records_the_partial_focus_with_its_round() {
    let mut tracker = tracker();
    let pomo = tracker
        .start_pomodoro(None, quick(false), None, at(0))
        .unwrap();
    let events = tracker.skip(pomo, at(4)).unwrap();
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(session.session.kind, TimerKind::Pomodoro);
    assert_eq!(session.session.round, Some(1));
    assert_eq!(session.session.effective, secs(4));
    assert!(matches!(
        events.last(),
        Some(TrackerEvent::Timer(TimersEvent::PomodoroPhaseEnded {
            next: Phase::ShortBreak,
            ..
        }))
    ));
}

#[test]
fn tick_writes_only_what_changed() {
    let mut tracker = tracker();
    let sw = tracker.start_stopwatch(None, at(0)).unwrap();
    let pomo = tracker
        .start_pomodoro(None, quick(true), None, at(0))
        .unwrap();
    break_writes(&tracker);
    // Nothing is due: no write is attempted, so the broken database does not matter.
    assert_eq!(tracker.tick(at(5)).unwrap(), vec![]);
    assert!(tracker.tick(at(10)).is_err()); // the focus ends: a session must be written
    assert!(tracker.timers().get(sw).is_some());
    assert!(tracker.timers().get(pomo).is_some());
}

type Command = Box<dyn Fn(&mut Tracker) -> Result<(), TrackerError>>;

#[test]
fn failed_writes_leave_memory_unchanged() {
    let mut tracker = tracker();
    let sw = tracker.start_stopwatch(None, at(0)).unwrap();
    let pomo = tracker
        .start_pomodoro(None, quick(false), None, at(0))
        .unwrap();
    let cd = tracker
        .start_countdown(None, secs(60), None, at(0))
        .unwrap();
    tracker.pause(cd, at(5)).unwrap();
    let paused_pomo = tracker
        .start_pomodoro(None, quick(false), None, at(0))
        .unwrap();
    let _events = tracker.skip(paused_pomo, at(1)).unwrap(); // short break waits
    let task = tracker.store_mut().create_task(None, "a", at(0)).unwrap();
    break_writes(&tracker);
    let before = tracker.timers().clone();

    let commands: Vec<Command> = vec![
        Box::new(|t| t.start_stopwatch(None, at(6)).map(drop)),
        Box::new(|t| t.start_countdown(None, secs(9), None, at(6)).map(drop)),
        Box::new(|t| t.start_pomodoro(None, quick(true), None, at(6)).map(drop)),
        Box::new(move |t| t.pause(sw, at(6))),
        Box::new(move |t| t.resume(cd, at(6))),
        Box::new(move |t| t.stop(sw, at(6)).map(drop)),
        Box::new(move |t| t.skip(pomo, at(6)).map(drop)),
        Box::new(move |t| t.start_phase(paused_pomo, at(6))),
        Box::new(move |t| t.set_task(sw, Some(task))),
        Box::new(move |t| t.discard(sw)),
        Box::new(|t| t.tick(at(100)).map(drop)),
    ];
    for (index, command) in commands.iter().enumerate() {
        let result = command(&mut tracker);
        assert!(
            matches!(result, Err(TrackerError::Store(StoreError::Sqlite(_)))),
            "command {index}"
        );
        assert_eq!(tracker.timers(), &before, "command {index}");
    }
}

#[test]
fn missing_task_is_refused_without_touching_memory() {
    let mut tracker = tracker();
    let id = tracker.start_stopwatch(None, at(0)).unwrap();
    let before = tracker.timers().clone();
    let error = tracker.set_task(id, Some(TaskId(999))).unwrap_err();
    assert!(matches!(error, TrackerError::Store(_)));
    assert!(error.to_string().starts_with("database error"));
    assert_eq!(tracker.timers(), &before);
    assert!(tracker.start_stopwatch(Some(TaskId(999)), at(1)).is_err());
    assert_eq!(tracker.timers(), &before);
}

#[test]
fn engine_errors_come_back_untouched() {
    let mut tracker = tracker();
    let sw = tracker.start_stopwatch(None, at(0)).unwrap();
    assert!(matches!(
        tracker.resume(sw, at(1)),
        Err(TrackerError::Engine(EngineError::AlreadyRunning))
    ));
    assert!(matches!(
        tracker.skip(sw, at(1)),
        Err(TrackerError::Engine(EngineError::NotPomodoro(_)))
    ));
    let missing = TimerId(99);
    let error = tracker.stop(missing, at(1)).unwrap_err();
    assert_eq!(error.to_string(), "timer #99 not found");
    assert!(tracker.discard(missing).is_err());
}

#[test]
fn task_swapped_mid_run_applies_to_the_whole_session() {
    let mut tracker = tracker();
    let first = tracker.store_mut().create_task(None, "a", at(0)).unwrap();
    let second = tracker.store_mut().create_task(None, "b", at(0)).unwrap();
    let id = tracker.start_stopwatch(Some(first), at(0)).unwrap();
    tracker.set_task(id, Some(second)).unwrap();
    assert_eq!(
        tracker.store().load_active().unwrap().1[0].task,
        Some(second)
    );
    let events = tracker.stop(id, at(60)).unwrap();
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(session.session.task, Some(second));
    assert_eq!(session.session.started_at, at(0));
    assert_eq!(tracker.store().task_time(first).unwrap(), Duration::ZERO);
}

#[test]
fn subtask_checked_during_the_timer_is_linked_on_stop() {
    let mut tracker = tracker();
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let subtask = tracker.store_mut().add_subtask(task, "outline").unwrap();
    let id = tracker.start_stopwatch(Some(task), at(0)).unwrap();
    tracker.complete_subtask(subtask, at(30), Some(id)).unwrap();
    tracker.reopen_subtask(subtask, at(40), Some(id)).unwrap();
    tracker.complete_subtask(subtask, at(50), Some(id)).unwrap();
    let events = tracker.stop(id, at(60)).unwrap();
    let detail = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(detail.events.len(), 3);
    assert!(detail.events.iter().all(|event| event.run == Some(id)));
}

#[test]
fn subtask_run_must_be_an_active_timer() {
    let mut tracker = tracker();
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let subtask = tracker.store_mut().add_subtask(task, "outline").unwrap();
    let missing = TimerId(5);
    assert!(matches!(
        tracker.complete_subtask(subtask, at(1), Some(missing)),
        Err(TrackerError::Engine(EngineError::TimerNotFound(_)))
    ));
    assert!(
        tracker
            .reopen_subtask(subtask, at(1), Some(missing))
            .is_err()
    );
    tracker.complete_subtask(subtask, at(1), None).unwrap();
    tracker.reopen_subtask(subtask, at(2), None).unwrap();
    assert_eq!(tracker.store().subtask_events(subtask).unwrap().len(), 2);
}

#[test]
fn deleting_a_task_or_preset_through_the_store_unlinks_the_timer() {
    let mut tracker = tracker();
    let task = tracker
        .store_mut()
        .create_task(None, "report", at(0))
        .unwrap();
    let preset = tracker
        .store_mut()
        .create_timer_preset("tea", mins(5), None)
        .unwrap();
    let id = tracker
        .start_countdown(Some(task), mins(5), Some(preset), at(0))
        .unwrap();
    tracker.store_mut().delete_task(task).unwrap();
    tracker.store_mut().delete_timer_preset(preset).unwrap();
    assert_eq!(tracker.tick(at(1)).unwrap(), vec![]);
    let active = tracker.timers().get(id).unwrap();
    assert_eq!((active.task, active.preset), (None, None));
    let events = tracker.stop(id, at(60)).unwrap();
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(session.session.task, None);
}

#[test]
fn forgotten_lists_running_stopwatches_past_the_limit() {
    let mut tracker = tracker();
    tracker.store_mut().set_forgotten_limit(mins(60)).unwrap();
    let running = tracker.start_stopwatch(None, at(0)).unwrap();
    let paused = tracker.start_stopwatch(None, at(0)).unwrap();
    tracker.pause(paused, at(3599)).unwrap();
    tracker
        .start_countdown(None, mins(600), None, at(0))
        .unwrap();
    assert_eq!(tracker.forgotten(at(3600)).unwrap(), vec![]);
    assert_eq!(tracker.forgotten(at(3601)).unwrap(), vec![running]);

    // "Record all" keeps every second; "Discard" records nothing.
    let events = tracker.stop(running, at(3601)).unwrap();
    let session = tracker.store().session(recorded(&events)[0]).unwrap();
    assert_eq!(session.session.effective, secs(3601));
    tracker.discard(paused).unwrap();
    assert!(tracker.timers().get(paused).is_none());
    assert_eq!(tracker.store().load_active().unwrap().1.len(), 1);

    tracker
        .store_mut()
        .set_setting("forgotten_limit_ms", "soon")
        .unwrap();
    assert!(tracker.forgotten(at(4000)).is_err());
}

#[test]
fn suspension_closes_overdue_phases_at_their_scheduled_time() {
    let mut tracker = tracker();
    let pomo = tracker
        .start_pomodoro(None, quick(true), None, at(0))
        .unwrap();
    let cd = tracker
        .start_countdown(None, secs(30), None, at(0))
        .unwrap();
    let sw = tracker.start_stopwatch(None, at(0)).unwrap();
    // A tick 5s late is still real time: the break starts on its own.
    let _events = tracker.tick(at(15)).unwrap();
    let Timer::Pomodoro(p) = &tracker.timers().get(pomo).unwrap().timer else {
        panic!("expected the pomodoro");
    };
    assert!(p.is_running());
    // The PC sleeps for an hour.
    let events = tracker.tick(at(3600)).unwrap();
    assert!(
        events.contains(&TrackerEvent::Timer(TimersEvent::CountdownFinished {
            timer: cd,
            at: at(30)
        }))
    );
    assert!(tracker.timers().get(cd).is_none());
    let Timer::Pomodoro(p) = &tracker.timers().get(pomo).unwrap().timer else {
        panic!("expected the pomodoro");
    };
    assert_eq!((p.phase(), p.focus_index()), (Phase::Focus, 2));
    assert!(p.is_waiting());
    let Timer::Stopwatch(watch) = &tracker.timers().get(sw).unwrap().timer else {
        panic!("expected the stopwatch");
    };
    assert_eq!(watch.elapsed(at(3600)), secs(3600));
    assert_eq!(recorded(&events).len(), 1); // the countdown; the break makes no session
}
