//! Reports (spec, section 6): overview, weekly, daily timeline and single session.
//!
//! Every period is in local time: the caller passes the zone (`chrono::Local` in the app).
//!
//! All views add up **session segments clipped to the period**, so a session that crosses a
//! period boundary (midnight, the start of a week, the edge of a custom range) is counted only
//! for the part that ran inside it. This refines the spec, where the overview summed whole
//! sessions: with clipping, the overview of a day and the weekly column of that day agree.

mod daily;
mod error;
mod overview;
mod period;
mod rows;
mod session;
mod weekly;

pub use daily::{Daily, SessionBlock, daily};
pub use error::ReportError;
pub use overview::{Overview, ProjectTotal, TaskTotal, overview};
pub use period::Period;
pub use rows::{ProjectInfo, SubtaskMarker, TaskInfo};
pub use session::{Milestone, Pause, SessionReport, session_report};
pub use weekly::{DayTotals, ProjectTime, Weekly, weekly};

#[cfg(test)]
mod test_support;
