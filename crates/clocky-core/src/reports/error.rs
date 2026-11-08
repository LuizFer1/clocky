//! Report errors.

use chrono::NaiveDate;

use crate::store::StoreError;
use crate::time::Timestamp;

/// Errors from building periods and reading reports.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// Reading the database failed, or a stored value is invalid.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The local midnight around this date cannot be represented.
    #[error("date {0} is out of range")]
    DateOutOfRange(NaiveDate),
    /// A custom period ends before it starts.
    #[error("invalid period: it ends ({end:?}) before it starts ({start:?})")]
    InvalidPeriod {
        /// Requested start.
        start: Timestamp,
        /// Requested end.
        end: Timestamp,
    },
}

impl From<rusqlite::Error> for ReportError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(StoreError::Sqlite(error))
    }
}
