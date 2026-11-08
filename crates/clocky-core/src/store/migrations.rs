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
///
/// Foreign keys are off while the scripts run, so rebuilding a table (create, copy, drop,
/// rename) does not cascade to its children; they are checked before the commit and turned back
/// on afterwards, on success and on error.
pub(super) fn run(conn: &mut Connection, migrations: &[&str]) -> Result<(), StoreError> {
    let current = check(conn, migrations)?;
    if current == latest(migrations)? {
        return Ok(());
    }
    conn.pragma_update(None, "foreign_keys", false)?;
    let applied = apply(conn, migrations, current);
    let restored = conn.pragma_update(None, "foreign_keys", true);
    applied?;
    restored?;
    Ok(())
}

fn apply(conn: &mut Connection, migrations: &[&str], current: u32) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    let mut version = current;
    for (target, script) in (1..).zip(migrations) {
        if target > current {
            tx.execute_batch(script)?;
            version = target;
        }
    }
    let violations: i64 =
        tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations > 0 {
        return Err(StoreError::Corrupt {
            what: "foreign keys after migration",
            value: format!("{violations} rows without their parent"),
        });
    }
    tx.pragma_update(None, "user_version", version)?;
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

    const PARENT_AND_CHILD: &str = "
        CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE child (
            id INTEGER PRIMARY KEY,
            parent_id INTEGER NOT NULL REFERENCES parent (id) ON DELETE CASCADE
        );
        INSERT INTO parent (id, name) VALUES (1, 'kept');
        INSERT INTO child (id, parent_id) VALUES (1, 1);";

    fn foreign_keys(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn rebuilding_a_parent_table_keeps_its_children() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        run(&mut conn, &[PARENT_AND_CHILD]).unwrap();
        let rebuild = "
            CREATE TABLE parent_new (id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT);
            INSERT INTO parent_new (id, name) SELECT id, name FROM parent;
            DROP TABLE parent;
            ALTER TABLE parent_new RENAME TO parent;";
        run(&mut conn, &[PARENT_AND_CHILD, rebuild]).unwrap();
        let children: i64 = conn
            .query_row("SELECT COUNT(*) FROM child", [], |row| row.get(0))
            .unwrap();
        assert_eq!(children, 1);
        assert_eq!(version(&conn), 2);
        assert_eq!(foreign_keys(&conn), 1);
    }

    #[test]
    fn foreign_key_violations_abort_the_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        run(&mut conn, &[PARENT_AND_CHILD]).unwrap();
        let orphan = "INSERT INTO child (id, parent_id) VALUES (2, 99);";
        assert!(matches!(
            run(&mut conn, &[PARENT_AND_CHILD, orphan]),
            Err(StoreError::Corrupt { .. })
        ));
        assert_eq!(version(&conn), 1);
        assert_eq!(foreign_keys(&conn), 1);
        let children: i64 = conn
            .query_row("SELECT COUNT(*) FROM child", [], |row| row.get(0))
            .unwrap();
        assert_eq!(children, 1);
    }
}
