//! Storage errors.

/// Errors from storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite failed: I/O, a violated constraint, invalid SQL.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database was written by a newer version of Clocky.
    #[error("database schema version {found} is newer than the supported version {supported}")]
    UnsupportedSchema {
        /// Version found in the file.
        found: u32,
        /// Latest version this build knows.
        supported: u32,
    },
    /// No row has this identifier.
    #[error("{entity} {id} not found")]
    NotFound {
        /// Kind of row (`"project"`, `"task"`...).
        entity: &'static str,
        /// The identifier that was not found.
        id: i64,
    },
    /// A number does not fit the target type (checked conversion).
    #[error("{0} is out of range")]
    OutOfRange(&'static str),
    /// A value read from the database is invalid.
    #[error("invalid {what} in the database: {value}")]
    Corrupt {
        /// What was being read.
        what: &'static str,
        /// The invalid value.
        value: String,
    },
}
