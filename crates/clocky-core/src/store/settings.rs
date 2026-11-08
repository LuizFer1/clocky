//! Settings: text key/value pairs, with typed accessors for the core's own settings.

use std::time::Duration;

use rusqlite::{OptionalExtension, params};

use super::convert::{duration_from_ms, duration_to_ms};
use super::{Store, StoreError};

/// Default stopwatch limit after which the app asks whether it was forgotten.
pub const DEFAULT_FORGOTTEN_LIMIT: Duration = Duration::from_hours(8);

const FORGOTTEN_LIMIT_KEY: &str = "forgotten_limit_ms";

impl Store {
    /// The value of a setting; `None` if it was never set.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        let value = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(value)
    }

    /// Sets a setting, replacing the previous value.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Stopwatch limit for the "forgotten?" dialog ([`DEFAULT_FORGOTTEN_LIMIT`] if never set).
    ///
    /// # Errors
    /// [`StoreError::Corrupt`] if the stored value is not a number; [`StoreError::OutOfRange`]
    /// if it is negative.
    pub fn forgotten_limit(&self) -> Result<Duration, StoreError> {
        let Some(text) = self.setting(FORGOTTEN_LIMIT_KEY)? else {
            return Ok(DEFAULT_FORGOTTEN_LIMIT);
        };
        let millis: i64 = text.parse().map_err(|_| StoreError::Corrupt {
            what: "forgotten limit",
            value: text.clone(),
        })?;
        duration_from_ms(millis, "forgotten limit")
    }

    /// Sets the stopwatch limit for the "forgotten?" dialog.
    ///
    /// # Errors
    /// [`StoreError::OutOfRange`] if `limit` does not fit in milliseconds.
    pub fn set_forgotten_limit(&mut self, limit: Duration) -> Result<(), StoreError> {
        let millis = duration_to_ms(limit, "forgotten limit")?;
        self.set_setting(FORGOTTEN_LIMIT_KEY, &millis.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::mins;

    #[test]
    fn missing_setting_is_none() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("theme").unwrap(), None);
    }

    #[test]
    fn set_replaces_the_value() {
        let mut store = Store::open_in_memory().unwrap();
        store.set_setting("theme", "dark").unwrap();
        store.set_setting("theme", "light").unwrap();
        assert_eq!(store.setting("theme").unwrap().as_deref(), Some("light"));
    }

    #[test]
    fn forgotten_limit_defaults_to_eight_hours() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.forgotten_limit().unwrap(), DEFAULT_FORGOTTEN_LIMIT);
        assert_eq!(DEFAULT_FORGOTTEN_LIMIT, mins(8 * 60));
    }

    #[test]
    fn forgotten_limit_roundtrip() {
        let mut store = Store::open_in_memory().unwrap();
        store.set_forgotten_limit(mins(90)).unwrap();
        assert_eq!(store.forgotten_limit().unwrap(), mins(90));
        assert!(matches!(
            store.set_forgotten_limit(Duration::MAX),
            Err(StoreError::OutOfRange("forgotten limit"))
        ));
    }

    #[test]
    fn corrupted_forgotten_limit_is_an_error() {
        let mut store = Store::open_in_memory().unwrap();
        store.set_setting(FORGOTTEN_LIMIT_KEY, "soon").unwrap();
        assert!(matches!(
            store.forgotten_limit(),
            Err(StoreError::Corrupt { .. })
        ));
        store.set_setting(FORGOTTEN_LIMIT_KEY, "-5").unwrap();
        assert!(matches!(
            store.forgotten_limit(),
            Err(StoreError::OutOfRange("forgotten limit"))
        ));
    }
}
