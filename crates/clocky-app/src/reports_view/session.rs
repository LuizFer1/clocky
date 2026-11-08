//! Sessão: one session with its effective time, pauses and milestones ("aos 18min").

use std::fmt::Display;
use std::time::Duration;

use chrono::{NaiveDate, TimeZone};
use clocky_core::engine::session::TimerKind;
use clocky_core::reports::SessionReport;
use clocky_core::store::SubtaskEventKind;
use clocky_core::time::Timestamp;
use slint::Color;

use super::{Axis, NO_TASK, project_label};
use crate::format::{self, dates};
use crate::ui;

/// The detail of one session.
#[derive(Debug, Clone)]
pub struct SessionView {
    pub task: String,
    pub project: String,
    pub color: Color,
    pub none: bool,
    /// "Pomodoro · foco 2"
    pub kind: String,
    /// "dom, 8 de novembro · 14:02 – 14:27"
    pub when: String,
    /// "25min"
    pub effective: String,
    /// "2 pausas · 6min" or "Sem pausas"
    pub pauses: String,
    /// Start to end, pauses included.
    pub wall: String,
    /// Running segments on the bar from start to end.
    pub parts: Vec<ui::TimelinePart>,
    pub marks: Vec<ui::TimelineMarker>,
    /// Start and end times under the bar.
    pub ticks: Vec<ui::HourTick>,
    pub events: Vec<ui::SessionEvent>,
}

pub fn build<Tz: TimeZone>(report: &SessionReport, today: NaiveDate, tz: &Tz) -> SessionView
where
    Tz::Offset: Display,
{
    let hh_mm = |at: Timestamp| format::hh_mm_in(at, tz);
    let (project, color, none) = project_label(report.project.as_ref());
    let date = tz
        .timestamp_millis_opt(report.started_at.as_millis())
        .earliest()
        .map_or(today, |t| t.date_naive());
    let pause_total: Duration = report.pauses.iter().map(|p| p.duration).sum();
    let (parts, marks) = bar(report, tz);
    SessionView {
        task: report
            .task
            .as_ref()
            .map_or(NO_TASK, |t| t.name.as_str())
            .to_owned(),
        project,
        color,
        none,
        kind: kind_text(report.kind, report.round),
        when: format!(
            "{} · {} – {}",
            dates::day_label(date, today),
            hh_mm(report.started_at),
            hh_mm(report.ended_at)
        ),
        effective: format::duration(report.effective),
        pauses: pauses_text(report.pauses.len(), pause_total),
        wall: format::duration(report.wall),
        parts,
        marks,
        ticks: vec![
            ui::HourTick {
                at: 0.0,
                label: hh_mm(report.started_at).into(),
            },
            ui::HourTick {
                at: 1.0,
                label: hh_mm(report.ended_at).into(),
            },
        ],
        events: events(report, tz),
    }
}

/// The running parts and the milestones on the bar from start to end.
fn bar<Tz: TimeZone>(
    report: &SessionReport,
    tz: &Tz,
) -> (Vec<ui::TimelinePart>, Vec<ui::TimelineMarker>)
where
    Tz::Offset: Display,
{
    let axis = Axis {
        start: report.started_at,
        end: report.ended_at,
    };
    let parts = report
        .segments
        .iter()
        .map(|s| ui::TimelinePart {
            start: axis.at(s.start),
            size: axis.len(s.start, s.end),
        })
        .collect();
    let marks = report
        .milestones
        .iter()
        .map(|m| ui::TimelineMarker {
            at: axis.at(m.marker.at),
            name: m.marker.name.as_str().into(),
            time: format::hh_mm_in(m.marker.at, tz).into(),
            done: m.marker.kind == SubtaskEventKind::Completed,
        })
        .collect();
    (parts, marks)
}

/// Start, pauses, milestones and end, by time.
fn events<Tz: TimeZone>(report: &SessionReport, tz: &Tz) -> Vec<ui::SessionEvent>
where
    Tz::Offset: Display,
{
    let hh_mm = |at: Timestamp| format::hh_mm_in(at, tz);
    let edge = |at: Timestamp, text: &str| event(ui::SessionEventKind::Edge, hh_mm(at), text, "");
    let mut events = vec![(report.started_at, 0, edge(report.started_at, "Início"))];
    for pause in &report.pauses {
        let time = format!("{} – {}", hh_mm(pause.start), hh_mm(pause.end));
        let detail = format::duration(pause.duration);
        let line = event(ui::SessionEventKind::Pause, time, "Pausa", &detail);
        events.push((pause.start, 1, line));
    }
    for milestone in &report.milestones {
        let marker = &milestone.marker;
        let kind = match marker.kind {
            SubtaskEventKind::Completed => ui::SessionEventKind::Done,
            SubtaskEventKind::Reopened => ui::SessionEventKind::Reopened,
        };
        let detail = format!("aos {}", format::duration(milestone.offset));
        let line = event(kind, hh_mm(marker.at), &marker.name, &detail);
        events.push((marker.at, 2, line));
    }
    events.push((report.ended_at, 3, edge(report.ended_at, "Fim")));
    // Same instant: start, pause, milestone, end.
    events.sort_by_key(|(at, order, _)| (*at, *order));
    events.into_iter().map(|(_, _, e)| e).collect()
}

fn event(kind: ui::SessionEventKind, time: String, text: &str, detail: &str) -> ui::SessionEvent {
    ui::SessionEvent {
        kind,
        time: time.into(),
        text: text.into(),
        detail: detail.into(),
    }
}

/// "Pomodoro · foco 2", "Cronômetro".
pub fn kind_text(kind: TimerKind, round: Option<u32>) -> String {
    match (kind, round) {
        (TimerKind::Pomodoro, Some(round)) => format!("Pomodoro · foco {round}"),
        (kind, _) => format::kind_label(kind).to_owned(),
    }
}

/// "Sem pausas", "1 pausa · 3min", "2 pausas · 6min".
fn pauses_text(count: usize, total: Duration) -> String {
    match count {
        0 => "Sem pausas".into(),
        1 => format!("1 pausa · {}", format::duration(total)),
        n => format!("{n} pausas · {}", format::duration(total)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_text_names_the_pomodoro_round() {
        assert_eq!(kind_text(TimerKind::Pomodoro, Some(2)), "Pomodoro · foco 2");
        assert_eq!(kind_text(TimerKind::Pomodoro, None), "Pomodoro");
        assert_eq!(kind_text(TimerKind::Stopwatch, None), "Cronômetro");
    }

    #[test]
    fn pauses_text_counts_and_sums() {
        let m = Duration::from_mins;
        assert_eq!(pauses_text(0, Duration::ZERO), "Sem pausas");
        assert_eq!(pauses_text(1, m(3)), "1 pausa · 3min");
        assert_eq!(pauses_text(2, m(6)), "2 pausas · 6min");
    }
}
