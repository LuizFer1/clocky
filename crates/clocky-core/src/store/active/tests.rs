use super::*;
use std::time::Duration;

use crate::domain::{PomodoroPresetId, TimerPresetId};
use crate::engine::countdown::Countdown;
use crate::engine::pomodoro::PomodoroConfig;
use crate::engine::stopwatch::Stopwatch;
use crate::store::NewPomodoroPreset;
use crate::test_util::{at, completed, mins};

fn stopwatch(id: u64, task: Option<TaskId>) -> ActiveTimer {
    let mut sw = Stopwatch::start(at(0));
    sw.pause(at(10)).unwrap();
    sw.resume(at(20)).unwrap();
    ActiveTimer::new(TimerId(id), task, None, Timer::Stopwatch(sw))
}

fn countdown(id: u64, preset: Option<TimerPresetId>) -> ActiveTimer {
    let mut cd = Countdown::start(at(0), mins(5));
    cd.pause(at(30)).unwrap();
    let preset = preset.map(PresetRef::Timer);
    ActiveTimer::new(TimerId(id), None, preset, Timer::Countdown(cd))
}

fn config() -> PomodoroConfig {
    PomodoroConfig::new(mins(25), mins(5), mins(15), 2, true).unwrap()
}

fn waiting_pomodoro(id: u64, preset: Option<PomodoroPresetId>) -> ActiveTimer {
    let mut p = Pomodoro::start(config(), at(0));
    let _events = p.tick(at(3600)); // late: the short break waits
    assert!(p.is_waiting());
    let preset = preset.map(PresetRef::Pomodoro);
    ActiveTimer::new(TimerId(id), None, preset, Timer::Pomodoro(p))
}

fn pomodoro_preset(store: &mut Store) -> PomodoroPresetId {
    store
        .create_pomodoro_preset(&NewPomodoroPreset {
            name: "classic",
            focus: mins(25),
            short_break: mins(5),
            long_break: mins(15),
            focus_per_round: 2,
            auto_start: true,
            task: None,
        })
        .unwrap()
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
fn every_kind_roundtrips() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    let timer_preset = store.create_timer_preset("tea", mins(5), None).unwrap();
    let pomodoro = pomodoro_preset(&mut store);
    let running = ActiveTimer::new(
        TimerId(4),
        Some(task),
        Some(PresetRef::Pomodoro(pomodoro)),
        Timer::Pomodoro(Pomodoro::start(config(), at(0))),
    );
    let saved = vec![
        stopwatch(1, Some(task)),
        countdown(2, Some(timer_preset)),
        waiting_pomodoro(3, Some(pomodoro)),
        running,
    ];
    for active in &saved {
        store.save_active(active).unwrap();
    }
    let (next, loaded) = store.load_active().unwrap();
    assert_eq!(loaded, saved);
    assert_eq!(next, 5);
}

#[test]
fn saving_again_replaces_the_row_and_its_segments() {
    let mut store = Store::open_in_memory().unwrap();
    let mut active = stopwatch(1, None);
    store.save_active(&active).unwrap();
    let Timer::Stopwatch(sw) = &mut active.timer else {
        panic!("expected a stopwatch");
    };
    sw.pause(at(40)).unwrap();
    store.save_active(&active).unwrap();
    assert_eq!(store.load_active().unwrap().1, vec![active]);
    assert_eq!(count(&store, "active_timers"), 1);
    assert_eq!(count(&store, "active_timer_segments"), 2);
}

#[test]
fn zero_length_segments_are_not_saved() {
    let mut store = Store::open_in_memory().unwrap();
    let mut sw = Stopwatch::start(at(10));
    sw.pause(at(10)).unwrap();
    let active = ActiveTimer::new(TimerId(1), None, None, Timer::Stopwatch(sw));
    store.save_active(&active).unwrap();
    assert_eq!(count(&store, "active_timer_segments"), 0);
    let (_, loaded) = store.load_active().unwrap();
    let Timer::Stopwatch(sw) = &loaded[0].timer else {
        panic!("expected a stopwatch");
    };
    assert_eq!(sw.elapsed(at(99)), Duration::ZERO);
    assert!(!sw.is_running());
}

#[test]
fn delete_removes_the_timer_and_its_segments() {
    let mut store = Store::open_in_memory().unwrap();
    store.save_active(&stopwatch(1, None)).unwrap();
    store.delete_active(TimerId(1)).unwrap();
    assert_eq!(count(&store, "active_timers"), 0);
    assert_eq!(count(&store, "active_timer_segments"), 0);
    assert!(matches!(
        store.delete_active(TimerId(1)),
        Err(StoreError::NotFound {
            entity: "active timer",
            id: 1
        })
    ));
}

#[test]
fn preset_of_the_wrong_kind_is_rejected() {
    let mut store = Store::open_in_memory().unwrap();
    let mut active = stopwatch(1, None);
    active.preset = Some(PresetRef::Timer(TimerPresetId(1)));
    assert!(matches!(
        store.save_active(&active),
        Err(StoreError::Invalid { .. })
    ));
    let mut active = countdown(2, None);
    active.preset = Some(PresetRef::Pomodoro(PomodoroPresetId(1)));
    let error = store.save_active(&active).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid preset of active timer: Pomodoro(PomodoroPresetId(1)) for timer #2"
    );
    assert_eq!(count(&store, "active_timers"), 0);
}

#[test]
fn deleting_a_task_or_preset_unlinks_the_timer() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    let timer_preset = store.create_timer_preset("tea", mins(5), None).unwrap();
    let pomodoro = pomodoro_preset(&mut store);
    assert_eq!(timer_preset.0, pomodoro.0); // same number, different tables
    store.save_active(&stopwatch(1, Some(task))).unwrap();
    store
        .save_active(&countdown(2, Some(timer_preset)))
        .unwrap();
    store
        .save_active(&waiting_pomodoro(3, Some(pomodoro)))
        .unwrap();

    store.delete_task(task).unwrap();
    store.delete_timer_preset(timer_preset).unwrap();
    let (_, loaded) = store.load_active().unwrap();
    assert_eq!(loaded[0].task, None);
    assert_eq!(loaded[1].preset, None);
    assert_eq!(loaded[2].preset, Some(PresetRef::Pomodoro(pomodoro)));

    store.delete_pomodoro_preset(pomodoro).unwrap();
    assert_eq!(store.load_active().unwrap().1[2].preset, None);
}

#[test]
fn next_id_follows_sessions_and_subtask_events() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(store.load_active().unwrap().0, 1);
    let task = store.create_task(None, "report", at(0)).unwrap();
    let subtask = store.add_subtask(task, "outline").unwrap();
    store.save_active(&stopwatch(3, None)).unwrap();
    assert_eq!(store.load_active().unwrap().0, 4);
    let span = completed(TimerKind::Stopwatch, &[(0, 10)]);
    store.record_session(None, TimerId(7), None, &span).unwrap();
    assert_eq!(store.load_active().unwrap().0, 8);
    store
        .complete_subtask(subtask, at(5), Some(TimerId(12)))
        .unwrap();
    assert_eq!(store.load_active().unwrap().0, 13);
}

#[test]
fn active_timers_of_lists_the_task_timers() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    store.save_active(&stopwatch(1, Some(task))).unwrap();
    store.save_active(&stopwatch(2, None)).unwrap();
    let mut pomodoro = waiting_pomodoro(3, None);
    pomodoro.task = Some(task);
    store.save_active(&pomodoro).unwrap();
    assert_eq!(
        store.active_timers_of(task).unwrap(),
        vec![
            ActiveTimerLink {
                timer: TimerId(1),
                kind: TimerKind::Stopwatch
            },
            ActiveTimerLink {
                timer: TimerId(3),
                kind: TimerKind::Pomodoro
            },
        ]
    );
}

/// Saves a valid pomodoro and a stopwatch, then runs `sql` to corrupt them.
fn corrupted(sql: &str) -> Result<(u64, Vec<ActiveTimer>), StoreError> {
    let mut store = Store::open_in_memory().unwrap();
    store.save_active(&stopwatch(1, None)).unwrap();
    store.save_active(&waiting_pomodoro(2, None)).unwrap();
    store.save_active(&countdown(3, None)).unwrap();
    store.execute_batch_for_tests(&format!(
        "PRAGMA ignore_check_constraints = ON;
         PRAGMA foreign_keys = OFF;
         DROP INDEX active_timer_segments_one_open;
         {sql}"
    ));
    store.load_active()
}

#[test]
fn corrupted_rows_are_errors() {
    let corrupt_cases = [
        "UPDATE active_timers SET kind = 'egg' WHERE id = 1",
        "UPDATE active_timers SET phase = 'nap' WHERE id = 2",
        "UPDATE active_timers SET focus_index = 0 WHERE id = 2",
        "UPDATE active_timers SET focus_ms = NULL WHERE id = 2",
        "UPDATE active_timers SET focus_ms = 10 WHERE id = 2",
        "UPDATE active_timers SET auto_start = 2 WHERE id = 2",
        "UPDATE active_timers SET target_ms = NULL WHERE id = 3",
        "INSERT INTO active_timer_segments VALUES (1, 100000, NULL)",
        "INSERT INTO active_timer_segments VALUES (1, 5000, 8000)",
        "INSERT INTO active_timer_segments VALUES (2, 0, 1000)",
        "UPDATE active_timer_segments SET end_ms = start_ms - 1 WHERE timer_id = 1",
    ];
    for sql in corrupt_cases {
        assert!(
            matches!(corrupted(sql), Err(StoreError::Corrupt { .. })),
            "{sql}"
        );
    }
    let out_of_range = [
        "UPDATE active_timers SET id = -1 WHERE id = 1",
        "UPDATE active_timers SET target_ms = -1 WHERE id = 3",
        "UPDATE active_timers SET round = -1 WHERE id = 2",
    ];
    for sql in out_of_range {
        assert!(
            matches!(corrupted(sql), Err(StoreError::OutOfRange(_))),
            "{sql}"
        );
    }
    assert!(corrupted("SELECT 1").is_ok());
}

#[test]
fn huge_run_id_cannot_produce_a_next_id() {
    let mut store = Store::open_in_memory().unwrap();
    let span = completed(TimerKind::Stopwatch, &[(0, 10)]);
    store.record_session(None, TimerId(1), None, &span).unwrap();
    store.execute_batch_for_tests("UPDATE sessions SET run_id = 9223372036854775807");
    assert!(store.load_active().is_ok());
    store.execute_batch_for_tests("UPDATE sessions SET run_id = -3");
    assert!(matches!(
        store.load_active(),
        Err(StoreError::OutOfRange("timer id"))
    ));
}

#[test]
fn a_missing_preset_is_refused_and_a_reused_id_is_not_linked() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(matches!(
        store.save_active(&countdown(1, Some(TimerPresetId(999)))),
        Err(StoreError::Sqlite(_))
    ));
    let old = store.create_timer_preset("tea", mins(5), None).unwrap();
    store.save_active(&countdown(1, Some(old))).unwrap();
    store.delete_timer_preset(old).unwrap();
    let new = store.create_timer_preset("coffee", mins(3), None).unwrap();
    assert_eq!(new, old); // SQLite reused the id
    assert_eq!(store.load_active().unwrap().1[0].preset, None);
}
