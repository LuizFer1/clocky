//! Semanal: one stacked column per day (Monday to Sunday) and the week's project totals.

use std::time::Duration;

use chrono::{Datelike, NaiveDate};
use clocky_core::reports::{DayTotals, Weekly};
use slint::{ModelRc, VecModel};

use super::{project_label, share};
use crate::format::{self, dates};
use crate::ui;

const HOUR: Duration = Duration::from_hours(1);

/// The chart and the legend.
#[derive(Debug, Clone)]
pub struct WeeklyView {
    pub days: Vec<ui::DayColumn>,
    /// Top and middle labels of the scale ("4h", "2h").
    pub scale_top: String,
    pub scale_mid: String,
    /// Time per project in the week, by time descending.
    pub projects: Vec<ui::ReportRow>,
}

pub fn build(weekly: &Weekly, today: NaiveDate) -> WeeklyView {
    let busiest = weekly
        .days
        .iter()
        .map(|d| d.total)
        .max()
        .unwrap_or_default();
    let half = scale_hours(busiest);
    let scale_len = HOUR * (half * 2);
    let days = weekly
        .days
        .iter()
        .map(|day| day_column(day, scale_len, today))
        .collect();
    let largest = weekly.projects.first().map(|p| p.total).unwrap_or_default();
    let projects = weekly
        .projects
        .iter()
        .map(|p| {
            let (name, color, none) = project_label(p.project.as_ref());
            ui::ReportRow {
                is_task: false,
                name: name.into(),
                color,
                none,
                archived: p.project.as_ref().is_some_and(|p| p.archived),
                total: format::duration(p.total).into(),
                share: "".into(),
                bar: share(p.total, largest),
            }
        })
        .collect();
    WeeklyView {
        days,
        scale_top: hours_label(half * 2),
        scale_mid: hours_label(half),
        projects,
    }
}

/// A day's column: its height on the scale and its slices.
fn day_column(day: &DayTotals, scale_len: Duration, today: NaiveDate) -> ui::DayColumn {
    // Top to bottom: the week's smallest project on top, the largest at the base.
    let parts: Vec<ui::StackPart> = day
        .projects
        .iter()
        .rev()
        .filter(|p| !p.total.is_zero())
        .map(|p| {
            let (_, color, none) = project_label(p.project.as_ref());
            ui::StackPart {
                color,
                none,
                size: share(p.total, day.total),
            }
        })
        .collect();
    ui::DayColumn {
        weekday: dates::weekday_short(day.date.weekday()).into(),
        day: day.date.day().to_string().into(),
        total: if day.total.is_zero() {
            "".into()
        } else {
            format::duration(day.total).into()
        },
        height: share(day.total, scale_len),
        parts: ModelRc::new(VecModel::from(parts)),
        today: day.date == today,
    }
}

/// Half of the chart scale, in hours, so the top is the busiest day rounded up to an even
/// number of hours (at least 2 h): the middle line then falls on a whole hour.
fn scale_hours(busiest: Duration) -> u32 {
    let hours = busiest.as_secs().div_ceil(3600);
    u32::try_from(hours.div_ceil(2).max(1)).unwrap_or(u32::MAX / 2)
}

/// "4h".
fn hours_label(hours: u32) -> String {
    format!("{hours}h")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_rounds_up_to_an_even_number_of_hours() {
        let h = |n: u64| Duration::from_mins(n);
        assert_eq!(scale_hours(Duration::ZERO), 1);
        assert_eq!(scale_hours(h(50)), 1);
        assert_eq!(scale_hours(h(120)), 1);
        assert_eq!(scale_hours(h(121)), 2);
        assert_eq!(scale_hours(h(250)), 3);
        assert_eq!(hours_label(scale_hours(h(250)) * 2), "6h");
        assert_eq!(hours_label(scale_hours(h(250))), "3h");
    }
}
