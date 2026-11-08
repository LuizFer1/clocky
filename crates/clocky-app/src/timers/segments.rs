//! Segment bar of the pomodoro cards (screen 40): one segment per phase of a round, focus in
//! the accent color and breaks in the pause color, widths proportional to the minutes.

use std::time::Duration;

use super::convert::ratio;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarPart {
    /// Share of the round, `0..=1`; the parts of a bar add up to 1.
    pub fraction: f32,
    pub focus: bool,
}

/// Phases of one round: `focus, short` × (n − 1), then `focus, long`.
pub fn segment_bar(focus: Duration, short: Duration, long: Duration, rounds: u32) -> Vec<BarPart> {
    let rounds = rounds.max(1);
    let total = focus.as_secs_f64() * f64::from(rounds)
        + short.as_secs_f64() * f64::from(rounds - 1)
        + long.as_secs_f64();
    let part = |d: Duration, focus: bool| BarPart {
        fraction: ratio(d.as_secs_f64(), total),
        focus,
    };
    let mut parts = Vec::new();
    for i in 1..=rounds {
        parts.push(part(focus, true));
        parts.push(if i == rounds {
            part(long, false)
        } else {
            part(short, false)
        });
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mins(m: u64) -> Duration {
        Duration::from_mins(m)
    }

    #[test]
    fn classic_round_has_eight_parts() {
        let bar = segment_bar(mins(25), mins(5), mins(15), 4);
        let kinds: Vec<bool> = bar.iter().map(|p| p.focus).collect();
        assert_eq!(kinds, [true, false, true, false, true, false, true, false]);
        // 4×25 + 3×5 + 15 = 130 minutes.
        assert!((bar[0].fraction - 25.0 / 130.0).abs() < 1e-6);
        assert!((bar[1].fraction - 5.0 / 130.0).abs() < 1e-6);
        assert!((bar[7].fraction - 15.0 / 130.0).abs() < 1e-6);
    }

    #[test]
    fn fractions_add_up_to_one() {
        let bar = segment_bar(mins(50), mins(10), mins(30), 3);
        let sum: f32 = bar.iter().map(|p| p.fraction).sum();
        assert!((sum - 1.0).abs() < 1e-5);
        assert_eq!(bar.len(), 6);
    }

    #[test]
    fn a_single_focus_goes_straight_to_the_long_break() {
        let bar = segment_bar(mins(25), mins(5), mins(15), 1);
        assert_eq!(bar.len(), 2);
        assert!((bar[0].fraction - 25.0 / 40.0).abs() < 1e-6);
        assert_eq!(segment_bar(mins(25), mins(5), mins(15), 0).len(), 2);
    }
}
