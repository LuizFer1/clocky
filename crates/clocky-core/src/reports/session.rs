//! Session report ("Sessão"): one session with its pauses and milestones.

use std::time::Duration;

use super::ReportError;
use super::rows::{ProjectInfo, SubtaskMarker, TaskInfo, add, subtask_markers};
use crate::domain::SessionId;
use crate::engine::run::Segment;
use crate::engine::session::TimerKind;
use crate::store::Store;
use crate::time::Timestamp;

/// One recorded session in detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReport {
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
    /// Start of the first segment.
    pub started_at: Timestamp,
    /// End of the last segment.
    pub ended_at: Timestamp,
    /// Running time (sum of the segments, pauses excluded).
    pub effective: Duration,
    /// Wall-clock time from start to end, pauses included.
    pub wall: Duration,
    /// Segments in which the timer ran, in order.
    pub segments: Vec<Segment>,
    /// Gaps between the segments, in order.
    pub pauses: Vec<Pause>,
    /// Subtask events linked to the session, oldest first.
    pub milestones: Vec<Milestone>,
}

/// A pause: the gap between two segments of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pause {
    /// End of the segment before.
    pub start: Timestamp,
    /// Start of the segment after.
    pub end: Timestamp,
    /// Length of the pause.
    pub duration: Duration,
}

/// A subtask event reached during a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    /// The event.
    pub marker: SubtaskMarker,
    /// Running time from the session start to the event ("aos 18min").
    pub offset: Duration,
}

/// The session `id` with its pauses and milestones.
///
/// A milestone's offset is the **running** time from the session start to the event: pauses
/// do not count, so a subtask checked after 18 minutes of focus reads "aos 18min" even if the
/// timer was paused before. An event during a pause gets the running time at which that pause
/// started. Only events linked to the session are milestones; events of the same timer outside
/// the session (for example during a pomodoro break) appear only in the daily timeline.
///
/// # Errors
/// [`ReportError::Store`] with [`crate::store::StoreError::NotFound`] if the session does not
/// exist, or if reading the database fails or a stored value is invalid.
pub fn session_report(store: &Store, id: SessionId) -> Result<SessionReport, ReportError> {
    let detail = store.session(id)?;
    let session = detail.session;
    let task = session.task.map(|task| store.task(task)).transpose()?;
    let project = task
        .as_ref()
        .and_then(|task| task.project)
        .map(|project| store.project(project))
        .transpose()?;
    let milestones = subtask_markers(store, "e.session_id = ?1", [id.0])?
        .into_iter()
        .map(|marker| Milestone {
            offset: running_time_until(&detail.segments, marker.at),
            marker,
        })
        .collect();
    Ok(SessionReport {
        session: id,
        kind: session.kind,
        round: session.round,
        task: task.map(|task| TaskInfo {
            id: task.id,
            name: task.name,
        }),
        project: project.map(|project| ProjectInfo {
            id: project.id,
            name: project.name,
            color: project.color,
            archived: project.archived_at.is_some(),
        }),
        started_at: session.started_at,
        ended_at: session.ended_at,
        effective: session.effective,
        wall: session.ended_at.since(session.started_at),
        pauses: pauses(&detail.segments),
        segments: detail.segments,
        milestones,
    })
}

/// The gaps between consecutive segments.
fn pauses(segments: &[Segment]) -> Vec<Pause> {
    segments
        .windows(2)
        .filter_map(|pair| match pair {
            [before, after] if after.start > before.end => Some(Pause {
                start: before.end,
                end: after.start,
                duration: after.start.since(before.end),
            }),
            _ => None,
        })
        .collect()
}

/// Running time from the first segment to `at`: the parts of the segments before `at`.
fn running_time_until(segments: &[Segment], at: Timestamp) -> Duration {
    let mut total = Duration::ZERO;
    for segment in segments {
        add(&mut total, segment.end.min(at).since(segment.start));
    }
    total
}

#[cfg(test)]
mod tests;
