//! Display text in the handoff's style (PT-BR). Pure functions: no clock, no time zone lookup.
//!
//! The manage window and the picker use the helpers here ("1h 05min", "25min"); the timer
//! panels use the clock displays and short durations of [`clock`] ("01:12:10", "12 min"), as
//! their handoff screens do.

mod clock;
// M7b: dates of the reports.
pub mod dates;
mod estimate;
pub mod markdown;
pub use clock::hh_mm_in;
pub use estimate::{estimate_input, estimate_progress, estimate_short, parse_estimate};

pub use clock::{
    active_count, duration_minutes, duration_short, elapsed_label, hh_mm_ss, local_hh_mm,
    pomodoro_durations, pomodoro_summary, remaining_label, session_recorded,
};

use std::time::Duration;

use chrono::{Datelike, NaiveDateTime, TimeZone};
use clocky_core::engine::session::TimerKind;
use clocky_core::time::Timestamp;

/// Placeholder for "nothing recorded" in the task rows.
pub const NO_TIME: &str = "—";

/// A duration as "1h 05min", "25min" or "45s" (seconds only under a minute; floors).
pub fn duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs == 0 {
        return "0min".into();
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    let (hours, minutes) = (secs / 3600, secs % 3600 / 60);
    if hours == 0 {
        format!("{minutes}min")
    } else {
        format!("{hours}h {minutes:02}min")
    }
}

/// Tracked time of a task row: [`duration`], or "—" when nothing was recorded.
pub fn tracked(d: Duration) -> String {
    if d.is_zero() {
        NO_TIME.into()
    } else {
        duration(d)
    }
}

/// "n/m".
pub fn ratio(done: u32, total: u32) -> String {
    format!("{done}/{total}")
}

/// "1/3 subtarefas" under a task name; empty when the task has no subtasks.
pub fn subtasks_label(done: u32, total: u32) -> String {
    if total == 0 {
        String::new()
    } else {
        format!("{} subtarefas", ratio(done, total))
    }
}

/// "1/3 concluídas" above a checklist.
pub fn checklist_label(done: u32, total: u32) -> String {
    format!("{} concluídas", ratio(done, total))
}

/// Share of `done` in `total`, from 0 to 1 (0 when there is nothing).
pub fn fraction(done: u32, total: u32) -> f32 {
    if total == 0 {
        return 0.0;
    }
    // Counts are small (a few thousand at most), so f32 is exact enough.
    #[expect(clippy::cast_precision_loss, reason = "small counts")]
    let value = done as f32 / total as f32;
    value.clamp(0.0, 1.0)
}

/// "4 abertas · 0 concluídas" (the handoff keeps the plural for 1).
pub fn task_counts(open: u32, completed: u32) -> String {
    format!("{open} abertas · {completed} concluídas")
}

/// "3 projetos" / "1 projeto".
pub fn project_count(count: usize) -> String {
    if count == 1 {
        "1 projeto".into()
    } else {
        format!("{count} projetos")
    }
}

/// Subtitle of the task view: "Aberta · 3h 40min registrados", "Aberta" or "Concluída".
pub fn task_status(completed: bool, tracked: Duration) -> String {
    match (completed, tracked.is_zero()) {
        (true, _) => "Concluída".into(),
        (false, true) => "Aberta".into(),
        (false, false) => format!("Aberta · {} registrados", duration(tracked)),
    }
}

/// Kind of a session, as in "Tempo registrado".
pub fn kind_label(kind: TimerKind) -> &'static str {
    match kind {
        TimerKind::Pomodoro => "Pomodoro",
        TimerKind::Stopwatch => "Cronômetro",
        TimerKind::Countdown => "Temporizador",
    }
}

/// Whole minutes of a duration (floors).
pub fn minutes(d: Duration) -> u64 {
    d.as_secs() / 60
}

/// "Clássico · 25/5/15" for a saved pomodoro.
pub fn pomodoro_link(name: &str, focus: Duration, short: Duration, long: Duration) -> String {
    let (f, s, l) = (minutes(focus), minutes(short), minutes(long));
    format!("{name} · {f}/{s}/{l}")
}

/// "pipeline · 12 min" for a saved countdown timer.
pub fn timer_link(name: &str, duration: Duration) -> String {
    format!("{name} · {} min", minutes(duration))
}

/// Day and time range of a session row: ("Hoje", "14:02 – 14:27"). Days are local to `tz`;
/// older days read "dd/mm".
pub fn session_when<Tz: TimeZone>(
    start: Timestamp,
    end: Timestamp,
    now: Timestamp,
    tz: &Tz,
) -> (String, String) {
    let (Some(start), Some(end), Some(now)) = (local(start, tz), local(end, tz), local(now, tz))
    else {
        return (String::new(), String::new());
    };
    let day = match (now.date() - start.date()).num_days() {
        0 => "Hoje".to_owned(),
        1 => "Ontem".to_owned(),
        _ => format!("{:02}/{:02}", start.day(), start.month()),
    };
    let range = format!("{} – {}", start.format("%H:%M"), end.format("%H:%M"));
    (day, range)
}

fn local<Tz: TimeZone>(at: Timestamp, tz: &Tz) -> Option<NaiveDateTime> {
    tz.timestamp_millis_opt(at.as_millis())
        .single()
        .map(|t| t.naive_local())
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    const MIN: u64 = 60;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn durations_follow_the_handoff() {
        assert_eq!(duration(secs(65 * MIN)), "1h 05min");
        assert_eq!(duration(secs(220 * MIN + 59)), "3h 40min");
        assert_eq!(duration(secs(25 * MIN)), "25min");
        assert_eq!(duration(secs(45)), "45s");
        assert_eq!(duration(secs(120 * MIN)), "2h 00min");
        assert_eq!(duration(Duration::ZERO), "0min");
        assert_eq!(duration(Duration::from_millis(999)), "0min");
    }

    #[test]
    fn tracked_time_uses_a_dash_for_nothing() {
        assert_eq!(tracked(Duration::ZERO), "—");
        assert_eq!(tracked(secs(45 * MIN)), "45min");
    }

    #[test]
    fn counts_and_ratios() {
        assert_eq!(ratio(1, 3), "1/3");
        assert_eq!(subtasks_label(1, 3), "1/3 subtarefas");
        assert_eq!(subtasks_label(0, 0), "");
        assert_eq!(checklist_label(2, 5), "2/5 concluídas");
        assert_eq!(task_counts(1, 1), "1 abertas · 1 concluídas");
        assert_eq!(project_count(3), "3 projetos");
        assert_eq!(project_count(1), "1 projeto");
        assert_eq!(project_count(0), "0 projetos");
    }

    #[test]
    fn fraction_is_bounded() {
        assert!((fraction(1, 2) - 0.5).abs() < f32::EPSILON);
        assert!(fraction(0, 0).abs() < f32::EPSILON);
        assert!((fraction(9, 3) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn task_status_lines() {
        assert_eq!(task_status(true, secs(MIN)), "Concluída");
        assert_eq!(task_status(false, Duration::ZERO), "Aberta");
        assert_eq!(
            task_status(false, secs(220 * MIN)),
            "Aberta · 3h 40min registrados"
        );
    }

    #[test]
    fn link_labels() {
        let m = |n| secs(n * MIN);
        assert_eq!(
            pomodoro_link("Clássico", m(25), m(5), m(15)),
            "Clássico · 25/5/15"
        );
        assert_eq!(timer_link("pipeline", m(12)), "pipeline · 12 min");
        assert_eq!(kind_label(TimerKind::Stopwatch), "Cronômetro");
        assert_eq!(kind_label(TimerKind::Pomodoro), "Pomodoro");
        assert_eq!(kind_label(TimerKind::Countdown), "Temporizador");
    }

    #[test]
    fn session_rows_are_local_days_and_times() {
        // 2026-11-08 14:02 at UTC-3 is 17:02 UTC.
        let tz = FixedOffset::west_opt(3 * 3600).unwrap();
        let at = |s: &str| {
            let t = chrono::DateTime::parse_from_rfc3339(s).unwrap();
            Timestamp::from_millis(t.timestamp_millis())
        };
        let now = at("2026-11-08T20:00:00-03:00");
        let today = session_when(
            at("2026-11-08T14:02:00-03:00"),
            at("2026-11-08T14:27:00-03:00"),
            now,
            &tz,
        );
        assert_eq!(today, ("Hoje".into(), "14:02 – 14:27".into()));
        let late = at("2026-11-07T23:30:00-03:00");
        assert_eq!(session_when(late, late, now, &tz).0, "Ontem");
        let old = at("2026-10-30T09:00:00-03:00");
        assert_eq!(session_when(old, old, now, &tz).0, "30/10");
    }
}
