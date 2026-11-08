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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_errors_become_store_errors() {
        let error = ReportError::from(rusqlite::Error::InvalidQuery);
        assert!(matches!(
            error,
            ReportError::Store(StoreError::Sqlite(rusqlite::Error::InvalidQuery))
        ));
        assert!(error.to_string().starts_with("database error"));
    }

    #[test]
    fn messages_name_the_bad_input() {
        let date = NaiveDate::MAX;
        assert_eq!(
            ReportError::DateOutOfRange(date).to_string(),
            format!("date {date} is out of range")
        );
        let invalid = ReportError::InvalidPeriod {
            start: Timestamp::from_millis(2),
            end: Timestamp::from_millis(1),
        };
        assert!(invalid.to_string().starts_with("invalid period"));
    }
}
