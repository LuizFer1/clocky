//! Reports (spec, section 6): overview, weekly, daily timeline and single session.
//!
//! Every period is in local time: the caller passes the zone (`chrono::Local` in the app).
//!
//! All views add up **session segments clipped to the period**, so a session that crosses a
//! period boundary (midnight, the start of a week, the edge of a custom range) is counted only
//! for the part that ran inside it. This refines the spec, where the overview summed whole
//! sessions: with clipping, the overview of a day and the weekly column of that day agree.

mod error;
mod overview;
mod period;
mod rows;

pub use error::ReportError;
pub use overview::{Overview, ProjectTotal, TaskTotal, overview};
pub use period::Period;
pub use rows::{ProjectInfo, TaskInfo};

#[cfg(test)]
mod test_support;
