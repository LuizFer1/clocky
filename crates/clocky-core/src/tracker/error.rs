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
/// ([`Store::clear_active_timers`]) before opening again.
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct TrackerOpenError {
    /// The store that was passed in, untouched by the failed open.
    pub store: Box<Store>,
    /// Why the timers could not be restored.
    #[source]
    pub error: TrackerError,
}
