//! Tracker errors.

use crate::engine::EngineError;
use crate::store::{Store, StoreError};

/// Errors from tracker commands. When one is returned, the timers in memory are unchanged.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    /// The engine refused the command (unknown timer, already paused...).
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The database failed or holds invalid data; nothing was written.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// [`crate::tracker::Tracker::open`] failed. The store is handed back, so the app can still
/// show the history and offer to discard the active timers
/// ([`TrackerOpenError::discard_active_timers`]) before opening again.
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct TrackerOpenError {
    /// The store that was passed in, untouched by the failed open.
    pub store: Box<Store>,
    /// Why the timers could not be restored.
    #[source]
    pub error: TrackerError,
}

impl TrackerOpenError {
    /// Deletes every active timer without recording anything (history is kept) and returns the
    /// store, ready for [`crate::tracker::Tracker::open`] again.
    ///
    /// # Errors
    /// [`StoreError::Sqlite`] if SQLite fails.
    pub fn discard_active_timers(self) -> Result<Store, StoreError> {
        let mut store = *self.store;
        store.clear_active_timers()?;
        Ok(store)
    }
}
