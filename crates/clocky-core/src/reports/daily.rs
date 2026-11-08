//! Daily timeline ("Diário"): session blocks and subtask markers of a local day.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{NaiveDate, TimeZone};

use super::rows::{
    ProjectInfo, SegmentRow, SubtaskMarker, TaskInfo, add, segments_overlapping, subtask_markers,
};
use super::{Period, ReportError};
use crate::domain::SessionId;
use crate::engine::run::Segment;
use crate::engine::session::TimerKind;
use crate::store::Store;

/// The timeline of one local day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Daily {
    /// The local date.
    pub date: NaiveDate,
    /// The day, from local midnight to the next one.
    pub period: Period,
    /// Sessions with time in the day, by their first segment.
    pub blocks: Vec<SessionBlock>,
    /// Subtasks checked or unchecked in the day, oldest first, with or without a session.
    pub markers: Vec<SubtaskMarker>,
    /// Time in the day.
    pub total: Duration,
}

/// One session on the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBlock {
    /// The session.
    pub session: SessionId,
    /// Kind of timer.
    pub kind: TimerKind,
    /// Pomodoro round (`None` for the other kinds).
    pub round: Option<u32>,
    /// Task; `None` = "No task".
    pub task: Option<TaskInfo>,
    /// Project of that task; `None` = "No project".
    pub project: Option<ProjectInfo>,
    /// Segments clipped to the day, in order; the gaps between them are the pauses.
    pub segments: Vec<Segment>,
    /// Time of the session in the day.
    pub total: Duration,
}

/// The timeline of the local day `date` in the zone `tz`.
///
/// Blocks hold the segments clipped to the day (a session that crosses midnight shows only its
/// part of the day). Markers are every subtask event of the day, including those with no
/// session (checked with no timer, or during a pomodoro break).
///
/// # Errors
/// [`ReportError::DateOutOfRange`] if the day cannot be represented; [`ReportError::Store`] if
/// reading the database fails or a stored value is invalid.
pub fn daily<Tz: TimeZone>(store: &Store, date: NaiveDate, tz: &Tz) -> Result<Daily, ReportError> {
    let period = Period::day(date, tz)?;
    let mut blocks: Vec<SessionBlock> = Vec::new();
    let mut index: BTreeMap<SessionId, usize> = BTreeMap::new();
    let mut total = Duration::ZERO;
    for row in segments_overlapping(store, period)? {
        let Some(part) = period.clip(row.segment) else {
            continue;
        };
        add(&mut total, part.duration());
        let at = *index.entry(row.session).or_insert_with(|| {
            blocks.push(SessionBlock::new(row));
            blocks.len() - 1
        });
        if let Some(block) = blocks.get_mut(at) {
            add(&mut block.total, part.duration());
            block.segments.push(part);
        }
    }
    let bounds = [period.start().as_millis(), period.end().as_millis()];
    let markers = subtask_markers(store, "e.at_ms >= ?1 AND e.at_ms < ?2", bounds)?;
    Ok(Daily {
        date,
        period,
        blocks,
        markers,
        total,
    })
}

impl SessionBlock {
    /// An empty block for the session of `row`.
    fn new(row: SegmentRow) -> Self {
        Self {
            session: row.session,
            kind: row.kind,
            round: row.round,
            task: row.task,
            project: row.project,
            segments: Vec::new(),
            total: Duration::ZERO,
        }
    }
}

#[cfg(test)]
mod tests;
