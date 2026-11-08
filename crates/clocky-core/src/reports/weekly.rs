//! Weekly ("Semanal"): time per day (Monday to Sunday) and project.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{Days, NaiveDate, TimeZone};

use super::period::{DAYS_PER_WEEK, monday_of};
use super::rows::{ProjectInfo, add, segments_overlapping, sort_by_total};
use super::{Period, ReportError};
use crate::domain::ProjectId;
use crate::store::Store;

/// Time per day and project in a local week.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weekly {
    /// The week, from Monday's local midnight to the next Monday's.
    pub period: Period,
    /// The 7 days, Monday first.
    pub days: Vec<DayTotals>,
    /// Time per project in the whole week, by time descending.
    pub projects: Vec<ProjectTime>,
    /// Time in the whole week.
    pub total: Duration,
}

/// Time per project in one local day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTotals {
    /// The local date.
    pub date: NaiveDate,
    /// The day (23 or 25 hours long on daylight saving changes).
    pub period: Period,
    /// Time per project in the day, in the order of [`Weekly::projects`] (the week's totals,
    /// descending), so every day lists its projects in the same order.
    pub projects: Vec<ProjectTime>,
    /// Time in the day.
    pub total: Duration,
}

/// Time of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTime {
    /// The project; `None` = "No project" (including sessions without a task).
    pub project: Option<ProjectInfo>,
    /// Its time.
    pub total: Duration,
}

/// Time per day and project in the local week (Monday to Sunday) that contains `date`.
///
/// `date` may be any day of the week: it is moved back to its Monday. Each day runs from local
/// midnight to local midnight, and segments are clipped to each day, so a session that crosses
/// midnight is split between the days (spec, decision 5). Every day lists its projects in the
/// order of [`Weekly::projects`], so a stacked chart keeps its series in the same order.
///
/// # Errors
/// [`ReportError::DateOutOfRange`] if a day of the week cannot be represented;
/// [`ReportError::Store`] if reading the database fails or a stored value is invalid.
pub fn weekly<Tz: TimeZone>(
    store: &Store,
    date: NaiveDate,
    tz: &Tz,
) -> Result<Weekly, ReportError> {
    let period = Period::week_of(date, tz)?;
    let rows = segments_overlapping(store, period)?;
    let mut week = ProjectTimes::default();
    let mut days = Vec::new();
    for (day_date, day) in week_days(date, tz)? {
        let mut times = ProjectTimes::default();
        for row in &rows {
            if let Some(part) = day.clip(row.segment) {
                times.add(row.project.as_ref(), part.duration());
                week.add(row.project.as_ref(), part.duration());
            }
        }
        days.push((day_date, day, times));
    }
    let (projects, total) = week.finish();
    let rank: BTreeMap<Option<ProjectId>, usize> = projects
        .iter()
        .enumerate()
        .map(|(rank, time)| (project_id(time), rank))
        .collect();
    let days = days
        .into_iter()
        .map(|(date, period, times)| {
            let mut projects: Vec<ProjectTime> = times.projects.into_values().collect();
            projects.sort_by_key(|time| rank.get(&project_id(time)).copied());
            DayTotals {
                date,
                period,
                projects,
                total: times.total,
            }
        })
        .collect();
    Ok(Weekly {
        period,
        days,
        projects,
        total,
    })
}

/// Identifier of the project of `time` (`None` = "No project").
fn project_id(time: &ProjectTime) -> Option<ProjectId> {
    time.project.as_ref().map(|project| project.id)
}

/// The 7 local days of the week that contains `date`, Monday first.
fn week_days<Tz: TimeZone>(
    date: NaiveDate,
    tz: &Tz,
) -> Result<Vec<(NaiveDate, Period)>, ReportError> {
    let monday = monday_of(date).ok_or(ReportError::DateOutOfRange(date))?;
    (0..DAYS_PER_WEEK)
        .map(|offset| {
            let day = monday
                .checked_add_days(Days::new(offset))
                .ok_or(ReportError::DateOutOfRange(date))?;
            Ok((day, Period::day(day, tz)?))
        })
        .collect()
}

/// Time per project being added up.
#[derive(Default)]
struct ProjectTimes {
    total: Duration,
    projects: BTreeMap<Option<ProjectId>, ProjectTime>,
}

impl ProjectTimes {
    fn add(&mut self, project: Option<&ProjectInfo>, time: Duration) {
        add(&mut self.total, time);
        let entry = self
            .projects
            .entry(project.map(|project| project.id))
            .or_insert_with(|| ProjectTime {
                project: project.cloned(),
                total: Duration::ZERO,
            });
        add(&mut entry.total, time);
    }

    /// The projects by time descending, and the total.
    fn finish(self) -> (Vec<ProjectTime>, Duration) {
        let mut projects: Vec<ProjectTime> = self.projects.into_values().collect();
        sort_by_total(&mut projects, |time| {
            (time.total, project_id(time).map(|id| id.0))
        });
        (projects, self.total)
    }
}

#[cfg(test)]
mod tests;
