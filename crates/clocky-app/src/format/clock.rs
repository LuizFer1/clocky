//! Clock displays and durations of the timer panels (re-exported by [`crate::format`]).
//!
//! Rounding: elapsed time rounds down (a second shows once it has passed), remaining time rounds
//! up (a fresh 12-minute countdown shows 12:00; 00:00 only once it is over).

use std::fmt::Display;
use std::time::Duration;

use chrono::{LocalResult, TimeZone};
use clocky_core::time::Timestamp;

/// Whole seconds rounded down: elapsed time shows a second only once it has passed.
pub fn secs_down(d: Duration) -> u64 {
    d.as_secs()
}

/// Whole seconds rounded up: a fresh 12-minute countdown shows 12:00, and 00:00 only appears
/// once it has finished.
pub fn secs_up(d: Duration) -> u64 {
    d.as_secs() + u64::from(d.subsec_nanos() > 0)
}

/// `HH:MM:SS`, the stopwatch display.
pub fn hh_mm_ss(total: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total / 60 % 60,
        total % 60
    )
}

/// `MM:SS`, or `H:MM:SS` from one hour on (countdowns and pomodoro phases).
pub fn mm_ss(total: u64) -> String {
    if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{:02}:{:02}", total / 60, total % 60)
    }
}

/// Elapsed time of a stopwatch (rounded down).
pub fn elapsed_label(d: Duration) -> String {
    hh_mm_ss(secs_down(d))
}

/// Remaining time of a countdown or pomodoro phase (rounded up).
pub fn remaining_label(d: Duration) -> String {
    mm_ss(secs_up(d))
}

/// Short duration: "12 min", "1 h 30 min", "2 h", "45 s", "1 min 30 s".
pub fn duration_short(d: Duration) -> String {
    let total = d.as_secs();
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h} h"));
    }
    if m > 0 {
        parts.push(format!("{m} min"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(format!("{s} s"));
    }
    parts.join(" ")
}

/// Like [`duration_short`], without the seconds once it reaches a minute ("1 h 12 min").
pub fn duration_minutes(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        return duration_short(d);
    }
    duration_short(Duration::from_secs(secs / 60 * 60))
}

/// Whole minutes of a pomodoro phase, rounded up (presets are made of whole minutes).
fn phase_minutes(d: Duration) -> u64 {
    secs_up(d).div_ceil(60)
}

/// "25 · 5 · 15 min", the pomodoro header subtitle.
pub fn pomodoro_durations(focus: Duration, short: Duration, long: Duration) -> String {
    format!(
        "{} · {} · {} min",
        phase_minutes(focus),
        phase_minutes(short),
        phase_minutes(long)
    )
}

/// "25 · 5 · 15 min · 4×", the pomodoro card summary.
pub fn pomodoro_summary(focus: Duration, short: Duration, long: Duration, rounds: u32) -> String {
    format!("{} · {rounds}×", pomodoro_durations(focus, short, long))
}

/// "Sessão de 01:12:10 registrada".
pub fn session_recorded(effective: Duration) -> String {
    format!("Sessão de {} registrada", elapsed_label(effective))
}

/// "4 timers · 0 alarmes" (alarms arrive in M7).
pub fn active_count(timers: usize, alarms: usize) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "{} · {}",
        plural(timers, "timer", "timers"),
        plural(alarms, "alarme", "alarmes")
    )
}

/// Local wall-clock `HH:MM` of `at`. It only converts the instant: the clock is never read here.
pub fn local_hh_mm(at: Timestamp) -> String {
    hh_mm_in(at, &chrono::Local)
}

/// `HH:MM` of `at` in `tz`.
pub fn hh_mm_in<Tz: TimeZone>(at: Timestamp, tz: &Tz) -> String
where
    Tz::Offset: Display,
{
    match tz.timestamp_millis_opt(at.as_millis()) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.format("%H:%M").to_string(),
        LocalResult::None => "--:--".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    #[test]
    fn remaining_rounds_up_and_elapsed_rounds_down() {
        let d = Duration::from_millis(719_400); // 11:59.4
        assert_eq!(remaining_label(d), "12:00");
        assert_eq!(elapsed_label(d), "00:11:59");
        assert_eq!(remaining_label(Duration::from_millis(1)), "00:01");
        assert_eq!(remaining_label(Duration::ZERO), "00:00");
    }

    #[test]
    fn clocks_pad_and_switch_to_hours() {
        assert_eq!(hh_mm_ss(4_330), "01:12:10");
        assert_eq!(mm_ss(59 * 60 + 59), "59:59");
        assert_eq!(mm_ss(3_600 + 65), "1:01:05");
    }

    #[test]
    fn short_durations_name_their_units() {
        assert_eq!(duration_short(Duration::from_mins(12)), "12 min");
        assert_eq!(duration_short(Duration::from_mins(90)), "1 h 30 min");
        assert_eq!(duration_short(Duration::from_hours(2)), "2 h");
        assert_eq!(duration_short(Duration::from_secs(45)), "45 s");
        assert_eq!(duration_short(Duration::from_secs(90)), "1 min 30 s");
        assert_eq!(duration_short(Duration::ZERO), "0 s");
    }

    #[test]
    fn minutes_drop_the_seconds() {
        assert_eq!(duration_minutes(Duration::from_secs(4_354)), "1 h 12 min");
        assert_eq!(duration_minutes(Duration::from_secs(42)), "42 s");
    }

    #[test]
    fn pomodoro_summary_matches_the_cards() {
        let s = pomodoro_summary(
            Duration::from_mins(25),
            Duration::from_mins(5),
            Duration::from_mins(15),
            4,
        );
        assert_eq!(s, "25 · 5 · 15 min · 4×");
    }

    #[test]
    fn counts_use_singular_and_plural() {
        assert_eq!(active_count(4, 0), "4 timers · 0 alarmes");
        assert_eq!(active_count(1, 1), "1 timer · 1 alarme");
    }

    #[test]
    fn session_notice_shows_the_effective_time() {
        assert_eq!(
            session_recorded(Duration::from_secs(65)),
            "Sessão de 00:01:05 registrada"
        );
    }

    #[test]
    fn wall_clock_uses_the_given_zone() {
        let brt = FixedOffset::west_opt(3 * 3600).unwrap();
        // 2026-11-08 17:32:00 UTC = 14:32 in UTC-3.
        let at = Timestamp::from_millis(1_794_159_120_000);
        assert_eq!(hh_mm_in(at, &brt), "14:32");
        assert_eq!(hh_mm_in(at, &chrono::Utc), "17:32");
    }
}
