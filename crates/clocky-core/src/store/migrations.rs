//! Schema migrations: SQL scripts applied in order, tracked by `PRAGMA user_version`.

use rusqlite::Connection;

use super::StoreError;
use super::convert::u32_from_sql;

/// Migration scripts. Script `i` (from 0) takes the schema from version `i` to `i + 1`.
pub(super) const MIGRATIONS: &[&str] = &[include_str!("sql/0001_init.sql")];

/// Latest schema version that `migrations` produces.
pub(super) fn latest(migrations: &[&str]) -> Result<u32, StoreError> {
    u32::try_from(migrations.len()).map_err(|_| StoreError::OutOfRange("migration count"))
}

/// Current schema version of the database.
///
/// # Errors
/// [`StoreError::UnsupportedSchema`] if it is newer than `migrations` knows.
pub(super) fn check(conn: &Connection, migrations: &[&str]) -> Result<u32, StoreError> {
    let raw: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let found = u32_from_sql(raw, "schema version")?;
    let supported = latest(migrations)?;
    if found > supported {
        return Err(StoreError::UnsupportedSchema { found, supported });
    }
    Ok(found)
}

/// Applies the pending scripts in a single transaction: if one fails, nothing changes.
pub(super) fn run(conn: &mut Connection, migrations: &[&str]) -> Result<(), StoreError> {
    let current = check(conn, migrations)?;
    let tx = conn.transaction()?;
    let mut version = current;
    for (target, script) in (1..).zip(migrations) {
        if target > current {
            tx.execute_batch(script)?;
            version = target;
        }
    }
    if version != current {
        tx.pragma_update(None, "user_version", version)?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "CREATE TABLE first (id INTEGER PRIMARY KEY);";
    const SECOND: &str = "CREATE TABLE second (id INTEGER PRIMARY KEY);";

    fn version(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    fn has_table(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    }

    #[test]
    fn applies_pending_scripts_in_order() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn, &[FIRST]).unwrap();
        assert_eq!(version(&conn), 1);
        run(&mut conn, &[FIRST, SECOND]).unwrap();
        assert_eq!(version(&conn), 2);
        assert!(has_table(&conn, "first"));
        assert!(has_table(&conn, "second"));
    }

    #[test]
    fn running_again_is_a_no_op() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn, &[FIRST]).unwrap();
        run(&mut conn, &[FIRST]).unwrap();
        assert_eq!(version(&conn), 1);
    }

    #[test]
    fn failed_migration_changes_nothing() {
        let mut conn = Connection::open_in_memory().unwrap();
        let broken = "CREATE TABLE second (id INTEGER PRIMARY KEY); NOT VALID SQL;";
        assert!(matches!(
            run(&mut conn, &[FIRST, broken]),
            Err(StoreError::Sqlite(_))
        ));
        assert_eq!(version(&conn), 0);
        assert!(!has_table(&conn, "first"));
        assert!(!has_table(&conn, "second"));
    }

    #[test]
    fn newer_schema_is_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();
        assert!(matches!(
            run(&mut conn, &[FIRST]),
            Err(StoreError::UnsupportedSchema {
                found: 5,
                supported: 1
            })
        ));
        assert!(!has_table(&conn, "first"));
    }

    #[test]
    fn negative_schema_version_is_an_error() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", -1).unwrap();
        assert!(matches!(
            run(&mut conn, &[FIRST]),
            Err(StoreError::OutOfRange("schema version"))
        ));
    }
}
