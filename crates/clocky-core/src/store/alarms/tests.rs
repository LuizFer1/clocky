use rusqlite::Connection;

use super::super::migrations::MIGRATIONS;
use crate::domain::TimerId;
use crate::engine::session::TimerKind;
use crate::store::{Store, backup_path};
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
