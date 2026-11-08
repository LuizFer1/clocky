use super::*;
use crate::test_util::{at, mins};

fn classic(task: Option<TaskId>) -> NewPomodoroPreset<'static> {
    NewPomodoroPreset {
        name: "classic",
        focus: mins(25),
        short_break: mins(5),
        long_break: mins(15),
        focus_per_round: 4,
        auto_start: false,
        task,
    }
}

#[test]
fn timer_presets_create_update_and_delete() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "tea", at(0)).unwrap();
    let id = store.create_timer_preset("tea", mins(3), None).unwrap();
    store
        .update_timer_preset(id, "green tea", mins(2), Some(task))
        .unwrap();
    assert_eq!(
        store.timer_presets().unwrap(),
        vec![TimerPreset {
            id,
            name: "green tea".into(),
            duration: mins(2),
            task: Some(task),
        }]
    );
    store.delete_timer_preset(id).unwrap();
    assert_eq!(store.timer_presets().unwrap().len(), 0);
}

#[test]
fn pomodoro_presets_create_update_and_delete() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "study", at(0)).unwrap();
    let id = store.create_pomodoro_preset(&classic(None)).unwrap();
    let changed = NewPomodoroPreset {
        auto_start: true,
        ..classic(Some(task))
    };
    store.update_pomodoro_preset(id, &changed).unwrap();
    assert_eq!(
        store.pomodoro_presets().unwrap(),
        vec![PomodoroPreset {
            id,
            name: "classic".into(),
            focus: mins(25),
            short_break: mins(5),
            long_break: mins(15),
            focus_per_round: 4,
            auto_start: true,
            task: Some(task),
        }]
    );
    store.delete_pomodoro_preset(id).unwrap();
    assert_eq!(store.pomodoro_presets().unwrap().len(), 0);
}

#[test]
fn schema_rejects_invalid_presets() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(matches!(
        store.create_timer_preset("zero", Duration::ZERO, None),
        Err(StoreError::Sqlite(_))
    ));
    let no_focus = NewPomodoroPreset {
        focus_per_round: 0,
        ..classic(None)
    };
    assert!(store.create_pomodoro_preset(&no_focus).is_err());
    assert!(
        store
            .create_timer_preset("x", mins(1), Some(TaskId(99)))
            .is_err()
    );
    assert!(matches!(
        store.create_timer_preset("huge", Duration::MAX, None),
        Err(StoreError::OutOfRange(_))
    ));
    let huge = NewPomodoroPreset {
        long_break: Duration::MAX,
        ..classic(None)
    };
    assert!(matches!(
        store.create_pomodoro_preset(&huge),
        Err(StoreError::OutOfRange("pomodoro long break"))
    ));
}

#[test]
fn deleting_the_task_unlinks_presets() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "tea", at(0)).unwrap();
    store
        .create_timer_preset("tea", mins(3), Some(task))
        .unwrap();
    store.create_pomodoro_preset(&classic(Some(task))).unwrap();
    store.delete_task(task).unwrap();
    assert_eq!(store.timer_presets().unwrap()[0].task, None);
    assert_eq!(store.pomodoro_presets().unwrap()[0].task, None);
}

#[test]
fn missing_presets_are_not_found() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(matches!(
        store.update_timer_preset(TimerPresetId(9), "x", mins(1), None),
        Err(StoreError::NotFound {
            entity: "timer preset",
            id: 9
        })
    ));
    assert!(store.delete_timer_preset(TimerPresetId(9)).is_err());
    let preset = classic(None);
    assert!(
        store
            .update_pomodoro_preset(PomodoroPresetId(9), &preset)
            .is_err()
    );
    assert!(store.delete_pomodoro_preset(PomodoroPresetId(9)).is_err());
}

#[test]
fn corrupted_presets_are_errors() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_timer_preset("tea", mins(3), None).unwrap();
    store.create_pomodoro_preset(&classic(None)).unwrap();
    store
        .conn
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE timer_presets SET duration_ms = -1;
             UPDATE pomodoro_presets SET auto_start = 2;",
        )
        .unwrap();
    assert!(matches!(
        store.timer_presets(),
        Err(StoreError::OutOfRange("timer preset duration"))
    ));
    assert!(matches!(
        store.pomodoro_presets(),
        Err(StoreError::Corrupt { .. })
    ));
}
