//! Estimated time of tasks and subtasks: what the user types ("1h30", "45min", "1:30", "90")
//! and how the estimate compares with the time recorded.

use std::time::Duration;

use super::duration;

/// The longest estimate accepted (also keeps the milliseconds far from overflowing).
const MAX_ESTIMATE: Duration = Duration::from_hours(10_000);

/// Typed text that is not an estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAnEstimate;

/// Parses a typed estimate; `Ok(None)` = no estimate (empty text).
///
/// Accepts minutes alone ("90"), "H:MM" ("1:30") and numbers with units, where hours may have
/// a decimal part ("1h30", "1h 30min", "1,5h", "45min", "45m"). Case and spaces are ignored.
pub fn parse_estimate(text: &str) -> Result<Option<Duration>, NotAnEstimate> {
    let text: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if text.is_empty() {
        return Ok(None);
    }
    let minutes = if let Some((h, m)) = text.split_once(':') {
        clock_minutes(h, m)
    } else if text.chars().all(|c| c.is_ascii_digit()) {
        text.parse::<f64>().ok()
    } else {
        unit_minutes(&text)
    };
    let estimate = minutes
        .and_then(|m| Duration::try_from_secs_f64(m * 60.0).ok())
        .ok_or(NotAnEstimate)?;
    // Whole minutes only: "1,33h" is 1h 19min.
    let estimate = Duration::from_secs(estimate.as_secs() / 60 * 60);
    if estimate.is_zero() || estimate > MAX_ESTIMATE {
        return Err(NotAnEstimate);
    }
    Ok(Some(estimate))
}

/// "1:30" in minutes; the minutes must have two digits and be under 60.
fn clock_minutes(hours: &str, minutes: &str) -> Option<f64> {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(hours) || !digits(minutes) || minutes.len() != 2 {
        return None;
    }
    let (h, m): (f64, f64) = (hours.parse().ok()?, minutes.parse().ok()?);
    (m < 60.0).then_some(h * 60.0 + m)
}

/// "1h30", "1h 30min", "1,5h" or "45min" in minutes. A number after "h" without a unit is
/// minutes ("1h30").
fn unit_minutes(text: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut rest = text;
    let mut seen_hours = false;
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == ',' || c == '.'))
            .unwrap_or(rest.len());
        let value: f64 = rest[..end].replace(',', ".").parse().ok()?;
        rest = &rest[end..];
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(rest.len());
        match &rest[..unit_end] {
            "h" if !seen_hours => {
                total += value * 60.0;
                seen_hours = true;
            }
            "min" | "m" => total += value,
            "" if seen_hours => total += value,
            _ => return None,
        }
        rest = &rest[unit_end..];
    }
    Some(total)
}

/// The estimate as the edit field shows it ("1h 30min"); empty without one.
pub fn estimate_input(estimate: Option<Duration>) -> String {
    estimate.map(duration).unwrap_or_default()
}

/// A short estimate for the task rows: whole hours as "4h", otherwise as [`duration`].
pub fn estimate_short(estimate: Duration) -> String {
    let secs = estimate.as_secs();
    if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else {
        duration(estimate)
    }
}

/// How far the recorded time is into the estimate: the bar fraction (0..1) and the label
/// ("75% · faltam 1h 00min", "Passou 15min da estimativa").
pub fn estimate_progress(tracked: Duration, estimate: Duration) -> (f32, String) {
    if estimate.is_zero() {
        return (0.0, String::new());
    }
    #[expect(clippy::cast_possible_truncation, reason = "a bar fraction")]
    let fraction = (tracked.as_secs_f64() / estimate.as_secs_f64()) as f32;
    if let Some(over) = tracked.checked_sub(estimate).filter(|d| !d.is_zero()) {
        return (1.0, format!("Passou {} da estimativa", duration(over)));
    }
    let percent = (fraction * 100.0).floor();
    let left = duration(estimate.saturating_sub(tracked));
    (fraction, format!("{percent:.0}% · faltam {left}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parsed estimate of `m` minutes.
    fn mins(m: u64) -> Result<Option<Duration>, NotAnEstimate> {
        parse_estimate(&m.to_string())
    }

    #[test]
    fn empty_text_clears_the_estimate() {
        assert_eq!(parse_estimate(""), Ok(None));
        assert_eq!(parse_estimate("   "), Ok(None));
    }

    #[test]
    fn plain_numbers_are_minutes() {
        assert_eq!(parse_estimate("90"), Ok(Some(Duration::from_mins(90))));
        assert_eq!(parse_estimate(" 45 "), mins(45));
    }

    #[test]
    fn units_and_clock_forms_are_understood() {
        assert_eq!(parse_estimate("1h30"), mins(90));
        assert_eq!(parse_estimate("1h 30min"), mins(90));
        assert_eq!(parse_estimate("1H 05MIN"), mins(65));
        assert_eq!(parse_estimate("2h"), mins(120));
        assert_eq!(parse_estimate("1,5h"), mins(90));
        assert_eq!(parse_estimate("0.25h"), mins(15));
        assert_eq!(parse_estimate("45min"), mins(45));
        assert_eq!(parse_estimate("45m"), mins(45));
        assert_eq!(parse_estimate("1:30"), mins(90));
        assert_eq!(parse_estimate("0:05"), mins(5));
    }

    #[test]
    fn what_the_field_shows_parses_back() {
        for m in [5, 45, 60, 65, 90, 600] {
            let shown = estimate_input(Some(Duration::from_mins(m)));
            assert_eq!(parse_estimate(&shown), mins(m), "{shown}");
        }
        assert_eq!(estimate_input(None), "");
    }

    #[test]
    fn nonsense_zero_and_huge_values_are_rejected() {
        for text in [
            "abc",
            "1x",
            "h",
            "1:3",
            "1:75",
            ":30",
            "0",
            "0min",
            "30s",
            "1h2h",
            "--",
            "1..5h",
            "99999999h",
        ] {
            assert_eq!(parse_estimate(text), Err(NotAnEstimate), "{text}");
        }
    }

    #[test]
    fn short_estimates_drop_zero_minutes() {
        assert_eq!(estimate_short(Duration::from_hours(4)), "4h");
        assert_eq!(estimate_short(Duration::from_mins(90)), "1h 30min");
        assert_eq!(estimate_short(Duration::from_mins(45)), "45min");
    }

    #[test]
    fn progress_counts_down_then_reports_the_overrun() {
        let hour = Duration::from_hours(1);
        let (fraction, label) = estimate_progress(Duration::from_mins(45), hour);
        assert!((fraction - 0.75).abs() < 1e-6);
        assert_eq!(label, "75% · faltam 15min");
        let (fraction, label) = estimate_progress(Duration::from_mins(75), hour);
        assert!((fraction - 1.0).abs() < 1e-6);
        assert_eq!(label, "Passou 15min da estimativa");
        assert_eq!(estimate_progress(hour, Duration::ZERO).1, "");
    }
}
