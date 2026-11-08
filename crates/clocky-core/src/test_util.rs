//! Test helpers.

use std::time::Duration;

use crate::time::Timestamp;

/// The instant `seconds` seconds after the Unix epoch.
pub(crate) fn at(seconds: i64) -> Timestamp {
    Timestamp::from_millis(seconds * 1000)
}

/// Duration of `n` seconds.
pub(crate) fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// Duration of `n` minutes.
pub(crate) fn mins(n: u64) -> Duration {
    Duration::from_mins(n)
}
