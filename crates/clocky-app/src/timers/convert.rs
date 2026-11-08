//! Conversions between the core types (`u64`, `i64`, `Duration`) and the Slint ones (`i32`,
//! `f32`). Every numeric cast of the timer panels lives here.
//!
//! Ids cross into Slint as `i32`; `-1` means "none" and never matches a real row.

use std::time::Duration;

use clocky_core::domain::TimerId;

/// The Slint value for "no id".
pub const NO_ID: i32 = -1;

pub fn timer_to_ui(id: TimerId) -> i32 {
    i32::try_from(id.0).unwrap_or(NO_ID)
}

pub fn timer_from_ui(value: i32) -> Option<TimerId> {
    u64::try_from(value).ok().map(TimerId)
}

/// A database row id (project, task, subtask, preset).
pub fn row_to_ui(id: i64) -> i32 {
    i32::try_from(id).unwrap_or(NO_ID)
}

pub fn row_from_ui(value: i32) -> Option<i64> {
    (value >= 0).then_some(i64::from(value))
}

/// `part / whole` clamped to `0..=1` (0 when `whole` is zero).
pub fn fraction(part: Duration, whole: Duration) -> f32 {
    if whole.is_zero() {
        return 0.0;
    }
    ratio(part.as_secs_f64(), whole.as_secs_f64())
}

/// `part / whole` as an `f32` in `0..=1`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a value in 0..=1 always fits an f32 (only precision is lost)"
)]
pub fn ratio(part: f64, whole: f64) -> f32 {
    if whole <= 0.0 {
        return 0.0;
    }
    (part / whole).clamp(0.0, 1.0) as f32
}

/// `part / whole` for counts ("1/3 concluídas").
pub fn count_ratio(part: usize, whole: usize) -> f32 {
    let as_f64 = |n: usize| f64::from(u32::try_from(n).unwrap_or(u32::MAX));
    ratio(as_f64(part), as_f64(whole))
}

/// Parses `#rrggbb` (the project colors). `None` for anything else.
pub fn hex_rgb(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// The project color for Slint; muted gray when the text is not a color.
pub fn color(text: &str) -> slint::Color {
    let (r, g, b) = hex_rgb(text).unwrap_or((0x8a, 0x94, 0xa6));
    slint::Color::from_rgb_u8(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_ids_roundtrip_and_overflow_to_none() {
        assert_eq!(timer_from_ui(timer_to_ui(TimerId(7))), Some(TimerId(7)));
        assert_eq!(timer_to_ui(TimerId(u64::MAX)), NO_ID);
        assert_eq!(timer_from_ui(NO_ID), None);
    }

    #[test]
    fn row_ids_roundtrip() {
        assert_eq!(row_from_ui(row_to_ui(42)), Some(42));
        assert_eq!(row_to_ui(i64::MAX), NO_ID);
        assert_eq!(row_from_ui(-1), None);
    }

    #[test]
    fn fractions_are_clamped() {
        let s = Duration::from_secs;
        assert!((fraction(s(30), s(120)) - 0.25).abs() < f32::EPSILON);
        assert!((fraction(s(200), s(120)) - 1.0).abs() < f32::EPSILON);
        assert!(fraction(s(5), Duration::ZERO).abs() < f32::EPSILON);
    }

    #[test]
    fn count_ratio_handles_empty_lists() {
        assert!((count_ratio(1, 4) - 0.25).abs() < f32::EPSILON);
        assert!(count_ratio(0, 0).abs() < f32::EPSILON);
    }

    #[test]
    fn hex_colors_parse() {
        assert_eq!(hex_rgb("#3d8fd6"), Some((0x3d, 0x8f, 0xd6)));
        assert_eq!(hex_rgb("3d8fd6"), None);
        assert_eq!(hex_rgb("#3d8fd"), None);
        assert_eq!(hex_rgb("#zzzzzz"), None);
        assert_eq!(hex_rgb("#ééé"), None);
    }
}
