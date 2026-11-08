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
    /// Time per project in the day, by time descending.
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

/// Time per day and project in the local week (Monday to Sunday) that contains `week_start`.
///
/// `week_start` may be any day of the week: it is moved back to its Monday. Each day runs from
/// local midnight to local midnight, and segments are clipped to each day, so a session that
/// crosses midnight is split between the days (spec, decision 5).
///
/// # Errors
/// [`ReportError::DateOutOfRange`] if a day of the week cannot be represented;
/// [`ReportError::Store`] if reading the database fails or a stored value is invalid.
pub fn weekly<Tz: TimeZone>(
    store: &Store,
    week_start: NaiveDate,
    tz: &Tz,
) -> Result<Weekly, ReportError> {
    let period = Period::week_of(week_start, tz)?;
    let days = week_days(week_start, tz)?;
    let rows = segments_overlapping(store, period)?;
    let mut week = ProjectTimes::default();
    let days = days
        .into_iter()
        .map(|(date, day)| {
            let mut times = ProjectTimes::default();
            for row in &rows {
                if let Some(part) = day.clip(row.segment) {
                    times.add(row.project.as_ref(), part.duration());
                    week.add(row.project.as_ref(), part.duration());
                }
            }
            let (projects, total) = times.finish();
            DayTotals {
                date,
                period: day,
                projects,
                total,
            }
        })
        .collect();
    let (projects, total) = week.finish();
    Ok(Weekly {
        period,
        days,
        projects,
        total,
    })
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
            (
                time.total,
                time.project.as_ref().map(|project| project.id.0),
            )
        });
        (projects, self.total)
    }
}

#[cfg(test)]
mod tests;
