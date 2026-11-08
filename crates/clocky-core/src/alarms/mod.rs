//! Alarm scheduling: pure rules, with no clock, no database and no I/O.
//!
//! The next ring is computed in local time (spec, section 5). Missed alarms follow
//! decision 4 of the spec.

pub mod alarm_time;
pub mod error;
pub mod weekdays;

pub use alarm_time::AlarmTime;
pub use error::AlarmError;
pub use weekdays::Weekdays;
