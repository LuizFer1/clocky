//! What the Relatórios page shows (spec, section 6), built from `clocky_core::reports`. Pure:
//! the caller passes today's date and the time zone, nothing here reads the clock.
//!
//! [`ReportNav`] is where the user is (tab, period, open session); [`build`] turns the core
//! report of that period into display rows whose sizes are 0..1 fractions.

mod daily;
mod overview;
mod session;
mod weekly;

use std::fmt::Display;
use std::time::Duration;

use chrono::{Datelike, Days, Months, NaiveDate, TimeZone};
use clocky_core::domain::{SessionId, TaskId};
use clocky_core::reports::{Period, ProjectInfo, ReportError};
use clocky_core::time::Timestamp;
use slint::Color;

use crate::core::AppCore;
use crate::format::{self, dates};
use crate::{palette, ui};

pub use daily::DailyView;
pub use session::SessionView;
pub use weekly::WeeklyView;

/// Label of the time without a project.
pub const NO_PROJECT: &str = "Sem projeto";
/// Label of the time without a task.
pub const NO_TASK: &str = "Sem tarefa";

/// The tab. The discriminant is `ReportsState.tab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview = 0,
    Weekly = 1,
    Daily = 2,
}

impl Tab {
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => Self::Overview,
            2 => Self::Daily,
            _ => Self::Weekly,
        }
    }
}

/// The period of Geral. The discriminant is `ReportsState.range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Day = 0,
    Week = 1,
    Month = 2,
}

impl Range {
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => Self::Day,
            2 => Self::Month,
            _ => Self::Week,
        }
    }
}

/// Where "‹ Voltar" of the Sessão detail goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// The Diário tab.
    Reports,
    /// The task page it was opened from ("Tempo registrado").
    Task(TaskId),
}

/// Where the user is in Relatórios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportNav {
    pub tab: Tab,
    pub range: Range,
    /// Any day of the period shown.
    pub date: NaiveDate,
    /// The open Sessão detail and where it goes back to.
    pub session: Option<(SessionId, Back)>,
}

impl ReportNav {
    /// The week of `today` (the first thing the page shows).
    pub fn new(today: NaiveDate) -> Self {
        Self {
            tab: Tab::Weekly,
            range: Range::Week,
            date: today,
            session: None,
        }
    }

    /// The unit of the period: the Geral range, a week or a day.
    pub fn unit(&self) -> Range {
        match self.tab {
            Tab::Overview => self.range,
            Tab::Weekly => Range::Week,
            Tab::Daily => Range::Day,
        }
    }

    /// Moves one period back or forward ("‹" / "›").
    pub fn shift(&mut self, forward: bool) {
        let date = self.date;
        let moved = match (self.unit(), forward) {
            (Range::Day, true) => date.checked_add_days(Days::new(1)),
            (Range::Day, false) => date.checked_sub_days(Days::new(1)),
            (Range::Week, true) => date.checked_add_days(Days::new(7)),
            (Range::Week, false) => date.checked_sub_days(Days::new(7)),
            (Range::Month, true) => date.checked_add_months(Months::new(1)),
            (Range::Month, false) => date.checked_sub_months(Months::new(1)),
        };
        self.date = moved.unwrap_or(date);
    }

    /// Whether the period holds `today` (or is after it).
    pub fn at_current(&self, today: NaiveDate) -> bool {
        self.bounds().1 >= today
    }

    /// "Hoje", "Esta semana" or "Este mês".
    pub fn current_label(&self) -> &'static str {
        match self.unit() {
            Range::Day => "Hoje",
            Range::Week => "Esta semana",
            Range::Month => "Este mês",
        }
    }

    /// First and last local dates of the period.
    pub fn bounds(&self) -> (NaiveDate, NaiveDate) {
        let date = self.date;
        match self.unit() {
            Range::Day => (date, date),
            Range::Week => {
                let back = u64::from(date.weekday().num_days_from_monday());
                let monday = date.checked_sub_days(Days::new(back)).unwrap_or(date);
                (
                    monday,
                    monday.checked_add_days(Days::new(6)).unwrap_or(monday),
                )
            }
            Range::Month => {
                let first = date.with_day(1).unwrap_or(date);
                let last = first
                    .checked_add_months(Months::new(1))
                    .and_then(|next| next.pred_opt())
                    .unwrap_or(first);
                (first, last)
            }
        }
    }

    /// The period in `tz`, from the first day's midnight to the midnight after the last day.
    ///
    /// # Errors
    /// [`ReportError::DateOutOfRange`] at the edges of the calendar.
    pub fn period<Tz: TimeZone>(&self, tz: &Tz) -> Result<Period, ReportError> {
        let (first, last) = self.bounds();
        Period::custom(
            Period::day(first, tz)?.start(),
            Period::day(last, tz)?.end(),
        )
    }

    /// "dom, 8 de novembro", "2 a 8 de novembro" or "novembro de 2026".
    pub fn period_label(&self, today: NaiveDate) -> String {
        let (first, last) = self.bounds();
        match self.unit() {
            Range::Day => dates::day_label(first, today),
            Range::Week => dates::range_label(first, last, today),
            Range::Month => dates::month_label(first),
        }
    }
}

/// Everything the Relatórios page shows for a [`ReportNav`].
#[derive(Debug, Clone)]
pub struct ReportsView {
    /// "Clocky / Relatórios / Semanal"
    pub crumb: String,
    pub heading: String,
    pub subheading: String,
    pub period_label: String,
    pub current_label: &'static str,
    pub at_current: bool,
    pub total: String,
    pub empty: bool,
    pub empty_title: &'static str,
    pub empty_text: &'static str,
    pub content: Content,
}

/// The body of the current tab, or the open session.
#[derive(Debug, Clone)]
pub enum Content {
    Overview(Vec<ui::ReportRow>),
    Weekly(WeeklyView),
    Daily(DailyView),
    Session(SessionView),
}

/// What the empty states say.
const EMPTY_TEXT: &str = "Inicie um pomodoro, cronômetro ou temporizador e o tempo aparece aqui.";

/// Builds the page for `nav`. `today` is the local date of the core's clock in `tz`.
///
/// # Errors
/// When the period cannot be represented or reading the reports fails.
pub fn build<Tz: TimeZone>(
    core: &AppCore,
    nav: &ReportNav,
    today: NaiveDate,
    tz: &Tz,
) -> Result<ReportsView, ReportError>
where
    Tz::Offset: Display,
{
    let mut view = ReportsView {
        crumb: "Relatórios".into(),
        heading: "Relatórios".into(),
        subheading: String::new(),
        period_label: nav.period_label(today),
        current_label: nav.current_label(),
        at_current: nav.at_current(today),
        total: String::new(),
        empty: false,
        empty_title: "",
        empty_text: EMPTY_TEXT,
        content: Content::Overview(Vec::new()),
    };
    if let Some((id, _)) = nav.session {
        let report = core.session_report(id)?;
        let detail = session::build(&report, today, tz);
        view.crumb = "Relatórios / Sessão".into();
        view.heading = "Sessão".into();
        view.subheading.clone_from(&detail.when);
        view.content = Content::Session(detail);
        return Ok(view);
    }
    let total = match nav.tab {
        Tab::Overview => {
            let overview = core.overview(nav.period(tz)?)?;
            view.crumb = "Relatórios / Geral".into();
            view.empty_title = match nav.range {
                Range::Day => "Nada registrado neste dia",
                Range::Week => "Nada registrado nesta semana",
                Range::Month => "Nada registrado neste mês",
            };
            view.content = Content::Overview(overview::rows(&overview));
            overview.total
        }
        Tab::Weekly => {
            let weekly = core.weekly(nav.date, tz)?;
            view.crumb = "Relatórios / Semanal".into();
            view.empty_title = "Nada registrado nesta semana";
            view.content = Content::Weekly(weekly::build(&weekly, today));
            weekly.total
        }
        Tab::Daily => {
            let daily = core.daily(nav.date, tz)?;
            view.crumb = "Relatórios / Diário".into();
            view.empty_title = "Nada registrado neste dia";
            view.empty = daily.blocks.is_empty() && daily.markers.is_empty();
            let total = daily.total;
            view.content = Content::Daily(daily::build(&daily, tz));
            total
        }
    };
    if nav.tab != Tab::Daily {
        view.empty = total.is_zero();
    }
    view.total = format::duration(total);
    view.subheading = format!("{} · {} registrados", view.period_label, view.total);
    Ok(view)
}

/// Name, color and "none" flag of a project (`None` = "Sem projeto").
fn project_label(project: Option<&ProjectInfo>) -> (String, Color, bool) {
    project.map_or_else(
        || (NO_PROJECT.to_owned(), Color::default(), true),
        |p| (p.name.clone(), palette::color(&p.color), false),
    )
}

/// `part / whole` as a 0..1 fraction (0 when `whole` is zero).
fn share(part: Duration, whole: Duration) -> f32 {
    if whole.is_zero() {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let ratio = (part.as_secs_f64() / whole.as_secs_f64()) as f32;
    ratio.clamp(0.0, 1.0)
}

/// Where instants fall on a horizontal axis from `start` to `end`.
#[derive(Debug, Clone, Copy)]
struct Axis {
    start: Timestamp,
    end: Timestamp,
}

impl Axis {
    fn at(self, at: Timestamp) -> f32 {
        share(at.since(self.start), self.end.since(self.start))
    }

    fn len(self, from: Timestamp, to: Timestamp) -> f32 {
        share(to.since(from), self.end.since(self.start))
    }
}

/// A database id as a Slint `int` (rows whose id does not fit are left out).
fn slint_id(id: i64) -> Option<i32> {
    crate::manage_view::slint_id(id)
}

#[cfg(test)]
mod tests;
