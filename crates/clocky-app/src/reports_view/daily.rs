//! Diário: the day's sessions on a timeline (one lane per overlap), the subtask markers and the
//! list of sessions under it.

use std::fmt::Display;

use chrono::{NaiveDate, NaiveTime, TimeZone, Timelike};
use clocky_core::reports::{Daily, SessionBlock, SubtaskMarker};
use clocky_core::store::SubtaskEventKind;
use clocky_core::time::Timestamp;
use slint::{ModelRc, VecModel};

use super::{Axis, NO_TASK, project_label, slint_id};
use crate::{format, ui};

/// Hours shown when the day has nothing to place.
const DEFAULT_HOURS: (u32, u32) = (8, 18);
/// Shortest span of the timeline, in hours.
const MIN_SPAN: u32 = 6;

/// The timeline and the list.
#[derive(Debug, Clone)]
pub struct DailyView {
    pub blocks: Vec<ui::TimelineBlock>,
    /// Number of lanes (sessions that overlap go on separate lanes).
    pub lanes: i32,
    pub markers: Vec<ui::TimelineMarker>,
    pub ticks: Vec<ui::HourTick>,
    pub sessions: Vec<ui::DailyRow>,
}

pub fn build<Tz: TimeZone>(daily: &Daily, tz: &Tz) -> DailyView
where
    Tz::Offset: Display,
{
    let (first, last) = hours(daily, tz);
    let axis = Axis {
        start: local_instant(daily.date, first, tz).unwrap_or(daily.period.start()),
        end: local_instant(daily.date, last, tz).unwrap_or(daily.period.end()),
    };
    let lanes = lanes(&daily.blocks);
    let blocks = daily
        .blocks
        .iter()
        .zip(&lanes)
        .filter_map(|(block, lane)| timeline_block(block, *lane, axis))
        .collect();
    let markers = daily
        .markers
        .iter()
        .map(|m| ui::TimelineMarker {
            at: axis.at(m.at),
            name: m.name.as_str().into(),
            time: format::hh_mm_in(m.at, tz).into(),
            done: m.kind == SubtaskEventKind::Completed,
        })
        .collect();
    let sessions = daily
        .blocks
        .iter()
        .filter_map(|block| daily_row(block, &daily.markers, tz))
        .collect();
    DailyView {
        blocks,
        lanes: lanes.iter().max().map_or(1, |l| l + 1),
        markers,
        // Placed by real time: on a 23 h or 25 h (DST) day the hours are not evenly spaced.
        ticks: ticks(first, last, |hour| {
            local_instant(daily.date, hour, tz).map(|at| axis.at(at))
        }),
        sessions,
    }
}

/// First and last whole local hours of the timeline: everything of the day fits, with at least
/// [`MIN_SPAN`] hours.
fn hours<Tz: TimeZone>(daily: &Daily, tz: &Tz) -> (u32, u32) {
    let instants = daily
        .blocks
        .iter()
        .flat_map(|b| b.segments.iter().flat_map(|s| [s.start, s.end]))
        .chain(daily.markers.iter().map(|m| m.at));
    let mut range: Option<(u32, u32)> = None;
    for at in instants {
        let Some((date, time)) = local(at, tz) else {
            continue;
        };
        // An end at the next midnight closes the day.
        let (low, high) = if date > daily.date {
            (24, 24)
        } else {
            let up = time.hour() + u32::from(time.minute() > 0 || time.second() > 0);
            (time.hour(), up)
        };
        range = Some(range.map_or((low, high), |(a, b)| (a.min(low), b.max(high))));
    }
    let (mut first, mut last) = range.unwrap_or(DEFAULT_HOURS);
    if last <= first {
        last = first + 1;
    }
    while last - first < MIN_SPAN {
        if last < 24 {
            last += 1;
        } else {
            first -= 1;
        }
    }
    (first, last)
}

fn local<Tz: TimeZone>(at: Timestamp, tz: &Tz) -> Option<(NaiveDate, NaiveTime)> {
    let local = tz.timestamp_millis_opt(at.as_millis()).earliest()?;
    let naive = local.naive_local();
    Some((naive.date(), naive.time()))
}

/// The instant of `hour`:00 on `date` in `tz` (24 = the next midnight).
fn local_instant<Tz: TimeZone>(date: NaiveDate, hour: u32, tz: &Tz) -> Option<Timestamp> {
    let (date, hour) = if hour >= 24 {
        (date.succ_opt()?, 0)
    } else {
        (date, hour)
    };
    let at = date.and_time(NaiveTime::from_hms_opt(hour, 0, 0)?);
    let local = tz.from_local_datetime(&at).earliest()?;
    Some(Timestamp::from_millis(local.timestamp_millis()))
}

/// Hour labels: every hour on short days, every 2 or 3 on longer ones. `place` gives the share of
/// the axis where an hour falls (`None` for an hour skipped by a DST change: no label).
fn ticks(first: u32, last: u32, place: impl Fn(u32) -> Option<f32>) -> Vec<ui::HourTick> {
    let span = last - first;
    let step = match span {
        0..=8 => 1,
        9..=16 => 2,
        _ => 3,
    };
    (first..=last)
        .step_by(step)
        .filter_map(|hour| {
            Some(ui::HourTick {
                at: place(hour)?,
                label: format!("{hour}h").into(),
            })
        })
        .collect()
}

/// A lane for each block: the first lane whose last block ended before it starts.
fn lanes(blocks: &[SessionBlock]) -> Vec<i32> {
    let mut ends: Vec<Timestamp> = Vec::new();
    blocks
        .iter()
        .map(|block| {
            let (Some(start), Some(end)) = (block.segments.first(), block.segments.last()) else {
                return 0;
            };
            let lane = ends.iter().position(|e| *e <= start.start);
            let lane = lane.unwrap_or_else(|| {
                ends.push(end.end);
                ends.len() - 1
            });
            ends[lane] = end.end;
            i32::try_from(lane).unwrap_or(0)
        })
        .collect()
}

fn timeline_block(block: &SessionBlock, lane: i32, axis: Axis) -> Option<ui::TimelineBlock> {
    let (first, last) = (block.segments.first()?, block.segments.last()?);
    let (start, end) = (first.start, last.end);
    let inner = Axis { start, end };
    let parts: Vec<ui::TimelinePart> = block
        .segments
        .iter()
        .map(|s| ui::TimelinePart {
            start: inner.at(s.start),
            size: inner.len(s.start, s.end),
        })
        .collect();
    let (_, color, none) = project_label(block.project.as_ref());
    Some(ui::TimelineBlock {
        id: slint_id(block.session.0)?,
        lane,
        start: axis.at(start),
        size: axis.len(start, end),
        color,
        none,
        parts: ModelRc::new(VecModel::from(parts)),
    })
}

fn daily_row<Tz: TimeZone>(
    block: &SessionBlock,
    markers: &[SubtaskMarker],
    tz: &Tz,
) -> Option<ui::DailyRow>
where
    Tz::Offset: Display,
{
    let (first, last) = (block.segments.first()?, block.segments.last()?);
    let (project, color, none) = project_label(block.project.as_ref());
    let markers: Vec<String> = markers
        .iter()
        .filter(|m| m.session == Some(block.session))
        .map(|m| marker_text(m, tz))
        .collect();
    Some(ui::DailyRow {
        id: slint_id(block.session.0)?,
        range: format!(
            "{} – {}",
            format::hh_mm_in(first.start, tz),
            format::hh_mm_in(last.end, tz)
        )
        .into(),
        task: block
            .task
            .as_ref()
            .map_or(NO_TASK, |t| t.name.as_str())
            .into(),
        project: project.into(),
        color,
        none,
        kind: super::session::kind_text(block.kind, block.round).into(),
        duration: format::duration(block.total).into(),
        markers: markers.join(" · ").into(),
    })
}

/// "✓ Endpoint de login às 14:20" (or "↺ … desmarcada às …").
fn marker_text<Tz: TimeZone>(marker: &SubtaskMarker, tz: &Tz) -> String
where
    Tz::Offset: Display,
{
    let at = format::hh_mm_in(marker.at, tz);
    match marker.kind {
        SubtaskEventKind::Completed => format!("✓ {} às {at}", marker.name),
        SubtaskEventKind::Reopened => format!("↺ {} desmarcada às {at}", marker.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_follow_real_time_and_skip_a_missing_hour() {
        // 0h to 6h on a day whose 1h repeats (25 h day): 7 real hours on the axis.
        let place = |hour: u32| {
            let real = if hour >= 2 { hour + 1 } else { hour };
            #[allow(clippy::cast_precision_loss)]
            Some(real as f32 / 7.0)
        };
        let long_day = ticks(0, 6, place);
        let at: Vec<f32> = long_day.iter().map(|t| t.at).collect();
        assert_eq!(at.len(), 7);
        assert!((at[2] - 3.0 / 7.0).abs() < 1e-6, "{at:?}");
        assert!((at[6] - 1.0).abs() < 1e-6, "{at:?}");
        // An hour skipped by the clock change gets no label.
        let gap = ticks(0, 6, |hour| (hour != 2).then_some(0.5));
        let labels: Vec<String> = gap.iter().map(|t| t.label.to_string()).collect();
        assert!(!labels.contains(&"2h".to_owned()), "{labels:?}");
        assert_eq!(labels.len(), 6);
    }
}
