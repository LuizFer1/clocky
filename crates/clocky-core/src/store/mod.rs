//! SQLite storage: opening, migrations and the repositories (one file per area).
//!
//! Every instant and duration is stored as `INTEGER` milliseconds. Writes that touch more than
//! one row run in a single transaction.

mod convert;
mod error;
mod migrations;

pub use error::StoreError;

use rusqlite::Connection;

/// Clocky database: one SQLite connection with the schema up to date.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
