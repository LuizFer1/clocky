//! Tracker errors.

use crate::engine::EngineError;
use crate::store::StoreError;

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
