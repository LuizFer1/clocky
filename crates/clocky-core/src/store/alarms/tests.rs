use chrono::Weekday;
use rusqlite::Connection;

use super::super::migrations::MIGRATIONS;
use super::test_support::{at_time, time, wake};
use super::*;
use crate::domain::TimerId;
use crate::engine::session::TimerKind;
use crate::store::backup_path;
use crate::test_util::{TempDb, at, completed, secs};

/// Inserts an alarm row with the given `repeat_days`, `snooze_minutes` and `enabled`.
fn insert_raw(store: &Store, repeat: i64, snooze: i64, enabled: i64) -> rusqlite::Result<usize> {
    store.conn.execute(
        "INSERT INTO alarms (name, time_local, repeat_days, snooze_minutes, enabled,
             created_at_ms) VALUES ('raw', '07:30', ?1, ?2, ?3, 0)",
        [repeat, snooze, enabled],
    )
}

#[test]
fn schema_has_the_alarms_table_with_its_checks() {
    let store = Store::open_in_memory().unwrap();
    insert_raw(&store, 127, 1, 1).unwrap();
    let (message, sound): (String, Option<String>) = store
        .conn
        .query_row("SELECT message, sound_path FROM alarms", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!((message.as_str(), sound), ("", None));
    assert!(insert_raw(&store, 128, 1, 1).is_err());
    assert!(insert_raw(&store, -1, 1, 1).is_err());
    assert!(insert_raw(&store, 0, 0, 1).is_err());
    assert!(insert_raw(&store, 0, 1, 2).is_err());
    let bad_time = store.conn.execute(
        "INSERT INTO alarms (name, time_local, repeat_days, snooze_minutes, enabled,
             created_at_ms) VALUES ('raw', '7:30', 0, 1, 1, 0)",
        [],
    );
    assert!(bad_time.is_err());
}

#[test]
fn migrating_a_version_1_file_keeps_its_data() {
    let db = TempDb::new("alarms-migrate-v1");
    let mut old = Store::open_with(db.path(), &MIGRATIONS[..1]).unwrap();
    assert_eq!(old.schema_version().unwrap(), 1);
    let project = old.create_project("work", "#000000", at(0)).unwrap();
    let task = old.create_task(Some(project), "report", at(1)).unwrap();
    let span = completed(TimerKind::Stopwatch, &[(10, 70)]);
    old.record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
    drop(old);

    let store = Store::open(db.path()).unwrap();
    assert_eq!(store.schema_version().unwrap(), 3);
    assert_eq!(store.task(task).unwrap().name, "report");
    assert_eq!(store.project(project).unwrap().name, "work");
    assert_eq!(store.task_time(task).unwrap(), secs(60));
    store
        .conn
        .execute(
            "INSERT INTO alarms (name, time_local, repeat_days, snooze_minutes, task_id,
                 enabled, created_at_ms) VALUES ('standup', '09:00', 31, 5, ?1, 1, 0)",
            [task.0],
        )
        .unwrap();

    let backup = Connection::open(backup_path(db.path())).unwrap();
    let version: i64 = backup
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
}

fn has_table(store: &Store, name: &str) -> bool {
    store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
        == 1
}

#[test]
fn a_version_1_file_migrates_through_active_timers_and_alarms() {
    let db = TempDb::new("alarms-migrate-v1-v3");
    let mut old = Store::open_with(db.path(), &MIGRATIONS[..1]).unwrap();
    let task = old.create_task(None, "report", at(1)).unwrap();
    assert!(!has_table(&old, "active_timers"));
    assert!(!has_table(&old, "alarms"));
    drop(old);

    let mut store = Store::open(db.path()).unwrap();
    assert_eq!(store.schema_version().unwrap(), 3);
    for table in ["active_timers", "active_timer_segments", "alarms"] {
        assert!(has_table(&store, table), "missing table {table}");
    }
    let alarm = store.create_alarm(&wake(Some(task)), at(2)).unwrap();
    assert_eq!(store.task_links(task).unwrap().alarms[0].id, alarm);
    assert_eq!(store.task_links(task).unwrap().active_timers.len(), 0);
}

#[test]
fn alarms_roundtrip_and_sort_by_time_then_id() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "standup", at(0)).unwrap();
    let late = store.create_alarm(&at_time("late", 9, 0), at(10)).unwrap();
    let early = store.create_alarm(&at_time("early", 7, 5), at(11)).unwrap();
    let full = NewAlarm {
        name: "meeting",
        time: time(9, 0),
        repeat: Weekdays::single(Weekday::Wed),
        sound: Some("C:/sounds/bell.ogg"),
        message: "Bring the report",
        snooze_minutes: 10,
        task: Some(task),
    };
    let tie = store.create_alarm(&full, at(12)).unwrap();

    assert_eq!(
        store.alarm(tie).unwrap(),
        Alarm {
            id: tie,
            name: "meeting".into(),
            time: time(9, 0),
            repeat: Weekdays::single(Weekday::Wed),
            sound: Some("C:/sounds/bell.ogg".into()),
            message: "Bring the report".into(),
            snooze_minutes: 10,
            task: Some(task),
            enabled: true,
            snoozed_until: None,
            last_fired_at: None,
            created_at: at(12),
        }
    );
    assert_eq!(ids(&store), vec![early, late, tie]);

    store.delete_alarm(late).unwrap();
    assert_eq!(ids(&store), vec![early, tie]);
}

fn ids(store: &Store) -> Vec<AlarmId> {
    store.alarms().unwrap().iter().map(|a| a.id).collect()
}

#[test]
fn missing_alarms_are_not_found() {
    let mut store = Store::open_in_memory().unwrap();
    let missing = AlarmId(9);
    let not_found = |result: Result<(), StoreError>| {
        matches!(
            result,
            Err(StoreError::NotFound {
                entity: "alarm",
                id: 9
            })
        )
    };
    assert!(not_found(store.alarm(missing).map(|_| ())));
    assert!(not_found(store.update_alarm(missing, &wake(None), at(0))));
    assert!(not_found(store.delete_alarm(missing)));
    assert!(not_found(store.set_alarm_enabled(missing, true, at(0))));
    assert!(not_found(store.set_alarm_enabled(missing, false, at(0))));
    assert!(not_found(store.set_alarm_task(missing, None)));
}

#[test]
fn schema_rejects_invalid_alarms() {
    let mut store = Store::open_in_memory().unwrap();
    let no_snooze = NewAlarm {
        snooze_minutes: 0,
        ..wake(None)
    };
    assert!(matches!(
        store.create_alarm(&no_snooze, at(0)),
        Err(StoreError::Sqlite(_))
    ));
    assert!(store.create_alarm(&wake(Some(TaskId(99))), at(0)).is_err());
    let id = store.create_alarm(&wake(None), at(0)).unwrap();
    assert!(store.update_alarm(id, &no_snooze, at(1)).is_err());
    assert!(store.set_alarm_task(id, Some(TaskId(99))).is_err());
}

/// Sets the snooze and the last fire of every alarm, bypassing the API.
fn force_schedule_state(store: &Store, snoozed_until: i64, last_fired_at: i64) {
    store
        .conn
        .execute(
            "UPDATE alarms SET snoozed_until_ms = ?1, last_fired_at_ms = ?2",
            [snoozed_until, last_fired_at],
        )
        .unwrap();
}

#[test]
fn updating_an_alarm_rearms_it() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store.create_alarm(&wake(None), at(0)).unwrap();
    store.set_alarm_enabled(id, false, at(1)).unwrap();
    force_schedule_state(&store, 5_000, 4_000);

    let changed = NewAlarm {
        repeat: Weekdays::ONCE,
        sound: Some("gong.wav"),
        message: "Go",
        ..at_time("gym", 6, 0)
    };
    store.update_alarm(id, &changed, at(100)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert_eq!(alarm.name, "gym");
    assert_eq!(alarm.time, time(6, 0));
    assert_eq!(alarm.repeat, Weekdays::ONCE);
    assert_eq!(alarm.sound.as_deref(), Some("gong.wav"));
    assert_eq!(alarm.message, "Go");
    assert_eq!(alarm.created_at, at(100));
    assert_eq!(alarm.snoozed_until, None);
    assert_eq!(alarm.last_fired_at, Some(Timestamp::from_millis(4_000)));
    assert!(!alarm.enabled, "editing keeps the enabled state");
}

#[test]
fn enabling_rearms_and_disabling_clears_the_snooze() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store.create_alarm(&wake(None), at(0)).unwrap();
    force_schedule_state(&store, 5_000, 4_000);

    store.set_alarm_enabled(id, true, at(50)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert_eq!(
        alarm.created_at,
        at(0),
        "enabling an enabled alarm changes nothing"
    );
    assert_eq!(alarm.snoozed_until, Some(Timestamp::from_millis(5_000)));

    store.set_alarm_enabled(id, false, at(60)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert!(!alarm.enabled);
    assert_eq!(alarm.snoozed_until, None);
    assert_eq!(alarm.created_at, at(0));

    store.set_alarm_enabled(id, true, at(70)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert!(alarm.enabled);
    assert_eq!(alarm.created_at, at(70));
    assert_eq!(alarm.last_fired_at, Some(Timestamp::from_millis(4_000)));
}

#[test]
fn alarms_link_to_tasks_and_survive_their_deletion() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    let id = store.create_alarm(&wake(None), at(0)).unwrap();
    store.set_alarm_task(id, Some(task)).unwrap();
    assert_eq!(store.alarm(id).unwrap().task, Some(task));
    store.set_alarm_task(id, None).unwrap();
    assert_eq!(store.alarm(id).unwrap().task, None);

    store.set_alarm_task(id, Some(task)).unwrap();
    store.delete_task(task).unwrap();
    assert_eq!(store.alarm(id).unwrap().task, None);
}

#[test]
fn schedule_copies_the_scheduling_fields() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store.create_alarm(&wake(None), at(0)).unwrap();
    force_schedule_state(&store, 5_000, 4_000);
    assert_eq!(
        store.alarm(id).unwrap().schedule(),
        AlarmSchedule {
            time: time(7, 30),
            repeat: Weekdays::WEEKDAYS,
            enabled: true,
            snoozed_until: Some(Timestamp::from_millis(5_000)),
            last_fired_at: Some(Timestamp::from_millis(4_000)),
            created_at: at(0),
        }
    );
}

#[test]
fn corrupted_alarms_are_errors() {
    for corruption in [
        "time_local = '7:30'",
        "time_local = '24:00'",
        "time_local = 730",
        "repeat_days = 200",
        "repeat_days = 300",
        "repeat_days = -1",
        "snooze_minutes = 0",
        "snooze_minutes = -5",
        "enabled = 2",
        "snoozed_until_ms = -1",
        "last_fired_at_ms = -1",
        "created_at_ms = -1",
    ] {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.create_alarm(&wake(None), at(0)).unwrap();
        store
            .conn
            .execute_batch(&format!(
                "PRAGMA ignore_check_constraints = ON; UPDATE alarms SET {corruption};"
            ))
            .unwrap();
        assert!(store.alarm(id).is_err(), "{corruption}");
        assert!(store.alarms().is_err(), "{corruption}");
    }
}

#[test]
fn linked_alarms_appear_in_task_links_and_summaries() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    let other = store.create_task(None, "gym", at(0)).unwrap();
    let late = store.create_alarm(&at_time("late", 9, 0), at(0)).unwrap();
    let early = store.create_alarm(&at_time("early", 8, 0), at(0)).unwrap();
    store.create_alarm(&wake(Some(other)), at(0)).unwrap();
    store.create_alarm(&wake(None), at(0)).unwrap();
    store.set_alarm_task(late, Some(task)).unwrap();
    store.set_alarm_task(early, Some(task)).unwrap();

    let linked: Vec<AlarmId> = store
        .task_links(task)
        .unwrap()
        .alarms
        .iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(linked, vec![early, late]);

    let summaries = store.task_summaries(None, false).unwrap();
    let alarm_counts: Vec<(TaskId, usize)> = summaries
        .iter()
        .map(|s| (s.task.id, s.links.alarms.len()))
        .collect();
    assert_eq!(alarm_counts, vec![(task, 2), (other, 1)]);
}

#[test]
fn schema_accepts_only_real_times_of_day() {
    let store = Store::open_in_memory().unwrap();
    let insert = |time: &str| {
        store.conn.execute(
            "INSERT INTO alarms (name, time_local, repeat_days, snooze_minutes, enabled,
                 created_at_ms) VALUES ('raw', ?1, 0, 1, 1, 0)",
            [time],
        )
    };
    for good in ["00:00", "09:59", "19:05", "20:00", "23:59"] {
        assert!(insert(good).is_ok(), "{good}");
    }
    for bad in ["24:00", "29:59", "7:30", "07:60", "07:5", "ab:cd"] {
        assert!(insert(bad).is_err(), "{bad}");
    }
}

#[test]
fn negative_instants_are_not_written() {
    let mut store = Store::open_in_memory().unwrap();
    let before_epoch = Timestamp::from_millis(-1);
    let out_of_range =
        |result: Result<(), StoreError>| matches!(result, Err(StoreError::OutOfRange(_)));
    assert!(out_of_range(
        store.create_alarm(&wake(None), before_epoch).map(|_| ())
    ));
    let id = store.create_alarm(&wake(None), at(0)).unwrap();
    assert!(out_of_range(store.update_alarm(
        id,
        &wake(None),
        before_epoch
    )));
    store.set_alarm_enabled(id, false, at(1)).unwrap();
    assert!(out_of_range(store.set_alarm_enabled(
        id,
        true,
        before_epoch
    )));
    assert!(out_of_range(store.mark_alarm_fired(
        id,
        before_epoch,
        at(2)
    )));
    store.set_alarm_enabled(id, true, at(3)).unwrap();
    let far_past = Timestamp::from_millis(-3_600_000);
    assert!(out_of_range(store.snooze_alarm(id, far_past).map(|_| ())));
    assert_eq!(store.alarm(id).unwrap().created_at, at(3));
}
