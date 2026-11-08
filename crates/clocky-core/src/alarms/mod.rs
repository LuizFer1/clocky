//! Alarm scheduling: pure rules, with no clock, no database and no I/O.
//!
//! The next ring is computed in local time (spec, section 5). Missed alarms follow
//! decision 4 of the spec.

pub mod alarm_time;
pub mod error;
pub mod firing;
pub mod next_fire;
pub mod schedule;
pub mod weekdays;

#[cfg(test)]
mod test_support;

pub use alarm_time::AlarmTime;
pub use error::AlarmError;
pub use firing::{Firing, LATE_TOLERANCE, ON_TIME_GRACE, classify, snooze_until};
pub use next_fire::next_fire;
pub use schedule::AlarmSchedule;
pub use weekdays::Weekdays;
