use chrono::{FixedOffset, TimeZone, Weekday};
use rusqlite::Connection;

use super::super::migrations::MIGRATIONS;
use super::*;
use crate::domain::TimerId;
use crate::engine::session::TimerKind;
use crate::store::backup_path;
use crate::test_util::{TempDb, at, completed, mins, secs};

fn time(hour: u8, minute: u8) -> AlarmTime {
    AlarmTime::new(hour, minute).unwrap()
}

/// A weekday alarm at 07:30, snoozing for 5 minutes.
fn wake(task: Option<TaskId>) -> NewAlarm<'static> {
    NewAlarm {
        name: "wake up",
        time: time(7, 30),
        repeat: Weekdays::WEEKDAYS,
        sound: None,
        message: "",
        snooze_minutes: 5,
        task,
    }
}

/// An alarm named `name` at `hour:minute`, otherwise like [`wake`].
fn at_time(name: &'static str, hour: u8, minute: u8) -> NewAlarm<'static> {
    NewAlarm {
        name,
        time: time(hour, minute),
        ..wake(None)
    }
}

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

/// Brasília time (UTC-3), with no daylight saving time.
fn brt() -> FixedOffset {
    FixedOffset::west_opt(3 * 3600).unwrap()
}

/// The instant at local `day` of November 2026 (the 2nd is a Monday), `hour:minute`, in BRT.
fn nov(day: u32, hour: u32, minute: u32) -> Timestamp {
    let local = brt()
        .with_ymd_and_hms(2026, 11, day, hour, minute, 0)
        .unwrap();
    Timestamp::from_millis(local.timestamp_millis())
}

/// Creates a daily alarm at `hour:minute`, armed at 06:00 on November 2nd.
fn daily(store: &mut Store, hour: u8, minute: u8) -> AlarmId {
    let alarm = NewAlarm {
        repeat: Weekdays::EVERY_DAY,
        ..at_time("daily", hour, minute)
    };
    store.create_alarm(&alarm, nov(2, 6, 0)).unwrap()
}

#[test]
fn pending_alarms_are_sorted_and_firing_handles_them() {
    let mut store = Store::open_in_memory().unwrap();
    let on_time = daily(&mut store, 10, 0);
    let late = daily(&mut store, 9, 50);
    let missed = daily(&mut store, 8, 0);
    daily(&mut store, 11, 0);
    let off = daily(&mut store, 9, 55);
    store.set_alarm_enabled(off, false, nov(2, 7, 0)).unwrap();

    let now = nov(2, 10, 0);
    let pending = store.pending_alarms(now, &brt()).unwrap();
    assert_eq!(
        pending,
        vec![
            (missed, nov(2, 8, 0), Firing::Missed),
            (late, nov(2, 9, 50), Firing::Late { by: mins(10) }),
            (on_time, nov(2, 10, 0), Firing::OnTime),
        ]
    );

    for (id, due, _) in pending {
        store.mark_alarm_fired(id, due).unwrap();
        let alarm = store.alarm(id).unwrap();
        assert_eq!(alarm.last_fired_at, Some(due));
        assert!(alarm.enabled, "a repeating alarm stays enabled");
    }
    assert_eq!(store.pending_alarms(now, &brt()).unwrap(), vec![]);
}

#[test]
fn once_alarm_is_disabled_after_firing_and_rings_again_when_rearmed() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    let pending = store.pending_alarms(nov(2, 10, 0), &brt()).unwrap();
    assert_eq!(pending, vec![(id, nov(2, 10, 0), Firing::OnTime)]);

    store.mark_alarm_fired(id, nov(2, 10, 0)).unwrap();
    assert!(!store.alarm(id).unwrap().enabled);
    assert_eq!(store.pending_alarms(nov(3, 10, 0), &brt()).unwrap(), vec![]);
    assert_eq!(store.next_alarm(nov(2, 10, 1), &brt()).unwrap(), None);

    let moved = NewAlarm {
        time: time(11, 0),
        ..once
    };
    store.update_alarm(id, &moved, nov(2, 10, 20)).unwrap();
    store.set_alarm_enabled(id, true, nov(2, 10, 30)).unwrap();
    let pending = store.pending_alarms(nov(2, 11, 0), &brt()).unwrap();
    assert_eq!(pending, vec![(id, nov(2, 11, 0), Firing::OnTime)]);
}

#[test]
fn snooze_survives_reopening_the_file() {
    let db = TempDb::new("alarms-snooze");
    let mut store = Store::open(db.path()).unwrap();
    let id = daily(&mut store, 10, 0);
    store.mark_alarm_fired(id, nov(2, 10, 0)).unwrap();
    let until = store.snooze_alarm(id, nov(2, 10, 1)).unwrap();
    assert_eq!(until, nov(2, 10, 6));
    drop(store);

    let mut store = Store::open(db.path()).unwrap();
    assert_eq!(store.alarm(id).unwrap().snoozed_until, Some(nov(2, 10, 6)));
    assert_eq!(store.pending_alarms(nov(2, 10, 3), &brt()).unwrap(), vec![]);
    assert_eq!(
        store.next_alarm(nov(2, 10, 3), &brt()).unwrap(),
        Some((id, nov(2, 10, 6)))
    );
    let pending = store.pending_alarms(nov(2, 10, 6), &brt()).unwrap();
    assert_eq!(pending, vec![(id, nov(2, 10, 6), Firing::OnTime)]);

    store.mark_alarm_fired(id, nov(2, 10, 6)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert_eq!(alarm.snoozed_until, None);
    assert_eq!(alarm.last_fired_at, Some(nov(2, 10, 6)));
}

#[test]
fn snoozing_a_fired_once_alarm_enables_it_until_the_snooze_rings() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        snooze_minutes: 9,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    store.mark_alarm_fired(id, nov(2, 10, 0)).unwrap();
    assert_eq!(
        store.snooze_alarm(id, nov(2, 10, 0)).unwrap(),
        nov(2, 10, 9)
    );
    assert!(store.alarm(id).unwrap().enabled);
    store.mark_alarm_fired(id, nov(2, 10, 9)).unwrap();
    assert!(!store.alarm(id).unwrap().enabled);
}

#[test]
fn next_alarm_is_the_earliest_enabled_one() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(store.next_alarm(nov(2, 7, 0), &brt()).unwrap(), None);
    daily(&mut store, 9, 0);
    let first = daily(&mut store, 8, 0);
    let off = daily(&mut store, 7, 30);
    store.set_alarm_enabled(off, false, nov(2, 7, 0)).unwrap();
    assert_eq!(
        store.next_alarm(nov(2, 7, 0), &brt()).unwrap(),
        Some((first, nov(2, 8, 0)))
    );
    assert_eq!(
        store.next_alarm(nov(2, 9, 30), &brt()).unwrap(),
        Some((first, nov(3, 8, 0)))
    );
}

#[test]
fn firing_or_snoozing_a_missing_alarm_is_not_found() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(matches!(
        store.mark_alarm_fired(AlarmId(9), nov(2, 10, 0)),
        Err(StoreError::NotFound {
            entity: "alarm",
            ..
        })
    ));
    assert!(matches!(
        store.snooze_alarm(AlarmId(9), nov(2, 10, 0)),
        Err(StoreError::NotFound {
            entity: "alarm",
            ..
        })
    ));
}

#[test]
fn a_corrupted_alarm_fails_the_runtime_operations() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    store
        .conn
        .execute_batch("PRAGMA ignore_check_constraints = ON; UPDATE alarms SET enabled = 7;")
        .unwrap();
    assert!(store.pending_alarms(nov(2, 10, 0), &brt()).is_err());
    assert!(store.next_alarm(nov(2, 10, 0), &brt()).is_err());
    assert!(store.mark_alarm_fired(id, nov(2, 10, 0)).is_err());
    assert!(store.snooze_alarm(id, nov(2, 10, 0)).is_err());
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
