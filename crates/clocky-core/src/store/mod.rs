//! SQLite storage: opening, migrations and the repositories (one file per area).
//!
//! Every instant and duration is stored as `INTEGER` milliseconds. Writes that touch more than
//! one row run in a single transaction.

mod convert;
mod error;
mod migrations;
mod presets;
mod projects;
mod rows;
mod settings;
mod subtasks;
mod tasks;

pub use error::StoreError;
pub use presets::{NewPomodoroPreset, PomodoroPreset, TimerPreset};
pub use projects::Project;
pub use settings::DEFAULT_FORGOTTEN_LIMIT;
pub use subtasks::{Subtask, SubtaskEvent, SubtaskEventKind};
pub use tasks::Task;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, MAIN_DB};

/// Clocky database: one SQLite connection with the schema up to date.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (or creates) the database file at `path` and applies the pending migrations.
    ///
    /// If the file already existed and a migration is pending, it is first copied to
    /// [`backup_path`]. A failed migration is rolled back, so the file is never left half
    /// migrated.
    ///
    /// # Errors
    /// [`StoreError::UnsupportedSchema`] if the file comes from a newer Clocky (the file is left
    /// untouched); [`StoreError::Sqlite`] if SQLite cannot open, back up or migrate it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let existed = path.exists();
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let current = migrations::check(&conn, migrations::MIGRATIONS)?;
        if existed && current < migrations::latest(migrations::MIGRATIONS)? {
            conn.backup(MAIN_DB, backup_path(path), None::<fn(_)>)?;
        }
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
        migrations::run(&mut conn, migrations::MIGRATIONS)?;
        Ok(Self { conn })
    }

    /// Opens an empty database in memory (tests and previews).
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", true)?;
        migrations::run(&mut conn, migrations::MIGRATIONS)?;
        Ok(Self { conn })
    }

    /// Schema version of the open database (the latest one once it is open).
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn schema_version(&self) -> Result<u32, StoreError> {
        migrations::check(&self.conn, migrations::MIGRATIONS)
    }
}

/// Path of the backup made before migrating `db`: the same name with `.bak` appended.
#[must_use]
pub fn backup_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDb;

    fn pragma_i64(store: &Store, name: &str) -> i64 {
        store
            .conn
            .pragma_query_value(None, name, |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn in_memory_store_is_migrated() {
        let store = Store::open_in_memory().unwrap();
        let latest = migrations::latest(migrations::MIGRATIONS).unwrap();
        assert_eq!(store.schema_version().unwrap(), latest);
    }

    #[test]
    fn schema_has_every_table() {
        let store = Store::open_in_memory().unwrap();
        for table in [
            "projects",
            "tasks",
            "subtasks",
            "timer_presets",
            "pomodoro_presets",
            "sessions",
            "session_segments",
            "subtask_events",
            "settings",
        ] {
            let count: i64 = store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing table {table}");
        }
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(pragma_i64(&store, "foreign_keys"), 1);
        let orphan = store.conn.execute(
            "INSERT INTO subtasks (task_id, name, position) VALUES (999, 'orphan', 1)",
            [],
        );
        assert!(orphan.is_err());
    }

    #[test]
    fn backup_path_appends_bak() {
        assert_eq!(
            backup_path(Path::new("data/clocky.db")),
            PathBuf::from("data/clocky.db.bak")
        );
    }

    #[test]
    fn new_file_uses_wal_and_reopens_without_backup() {
        let db = TempDb::new("store-new-file");
        let store = Store::open(db.path()).unwrap();
        let mode: String = store
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        store
            .conn
            .execute(
                "INSERT INTO settings (key, value) VALUES ('kept', 'yes')",
                [],
            )
            .unwrap();
        drop(store);

        let store = Store::open(db.path()).unwrap();
        let kept: String = store
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'kept'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(kept, "yes");
        assert_eq!(pragma_i64(&store, "foreign_keys"), 1);
        assert!(!backup_path(db.path()).exists());
    }

    #[test]
    fn existing_file_with_pending_migration_is_backed_up() {
        let db = TempDb::new("store-backup");
        let old = Connection::open(db.path()).unwrap();
        old.execute_batch("CREATE TABLE legacy (id INTEGER PRIMARY KEY);")
            .unwrap();
        drop(old);

        let store = Store::open(db.path()).unwrap();
        let latest = migrations::latest(migrations::MIGRATIONS).unwrap();
        assert_eq!(store.schema_version().unwrap(), latest);

        let backup = Connection::open(backup_path(db.path())).unwrap();
        let version: i64 = backup
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
        let legacy: i64 = backup
            .query_row("SELECT COUNT(*) FROM legacy", [], |row| row.get(0))
            .unwrap();
        assert_eq!(legacy, 0);
    }

    #[test]
    fn newer_schema_leaves_the_file_untouched() {
        let db = TempDb::new("store-newer");
        let newer = Connection::open(db.path()).unwrap();
        newer
            .execute_batch(
                "CREATE TABLE future (id INTEGER PRIMARY KEY); PRAGMA user_version = 99;",
            )
            .unwrap();
        drop(newer);
        let before = std::fs::read(db.path()).unwrap();

        assert!(matches!(
            Store::open(db.path()),
            Err(StoreError::UnsupportedSchema { found: 99, .. })
        ));
        assert_eq!(std::fs::read(db.path()).unwrap(), before);
        assert!(!backup_path(db.path()).exists());
    }
}
