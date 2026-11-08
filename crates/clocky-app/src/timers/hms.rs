//! The h/m/s editor of the Temporizador configuration (screen 20).

use std::time::Duration;

/// Largest countdown the editor builds: 23:59:59.
pub const MAX_SECS: u64 = 23 * 3600 + 59 * 60 + 59;

/// Field of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Hours,
    Minutes,
    Seconds,
}

impl Field {
    /// Field from the Slint index (0 = hours, 1 = minutes, 2 = seconds).
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Hours),
            1 => Some(Self::Minutes),
            2 => Some(Self::Seconds),
            _ => None,
        }
    }
}

/// Hours, minutes and seconds of the countdown being configured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hms {
    pub h: u32,
    pub m: u32,
    pub s: u32,
}

impl Hms {
    /// Splits `total` seconds, clamped to [`MAX_SECS`].
    pub fn from_secs(total: u64) -> Self {
        let total = total.min(MAX_SECS);
        // Each part is below 60 (hours below 24), so the conversions never fail.
        let part = |v: u64| u32::try_from(v).unwrap_or(0);
        Self {
            h: part(total / 3600),
            m: part(total / 60 % 60),
            s: part(total % 60),
        }
    }

    pub fn total_secs(self) -> u64 {
        u64::from(self.h) * 3600 + u64::from(self.m) * 60 + u64::from(self.s)
    }

    pub fn duration(self) -> Duration {
        Duration::from_secs(self.total_secs())
    }

    /// "Iniciar" is disabled at zero.
    pub fn is_zero(self) -> bool {
        self.total_secs() == 0
    }

    /// One arrow click: the field wraps around on its own (59 → 0, 0 → 23 hours).
    #[must_use]
    pub fn step(self, field: Field, up: bool) -> Self {
        let wrap = |value: u32, modulo: u32| {
            if up {
                (value + 1) % modulo
            } else {
                (value + modulo - 1) % modulo
            }
        };
        match field {
            Field::Hours => Self {
                h: wrap(self.h, 24),
                ..self
            },
            Field::Minutes => Self {
                m: wrap(self.m, 60),
                ..self
            },
            Field::Seconds => Self {
                s: wrap(self.s, 60),
                ..self
            },
        }
    }

    /// The +1/+5/+10 min shortcuts: minutes carry into hours, clamped to [`MAX_SECS`].
    #[must_use]
    pub fn add_minutes(self, minutes: u32) -> Self {
        Self::from_secs(self.total_secs() + u64::from(minutes) * 60)
    }

    /// Two-digit texts for the three fields.
    pub fn texts(self) -> [String; 3] {
        [self.h, self.m, self.s].map(|v| format!("{v:02}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn hms(h: u32, m: u32, s: u32) -> Hms {
        Hms { h, m, s }
    }

    #[test]
    fn arrows_wrap_inside_their_field() {
        assert_eq!(hms(0, 59, 0).step(Field::Minutes, true), hms(0, 0, 0));
        assert_eq!(hms(0, 0, 0).step(Field::Seconds, false), hms(0, 0, 59));
        assert_eq!(hms(23, 5, 5).step(Field::Hours, true), hms(0, 5, 5));
        assert_eq!(hms(0, 5, 5).step(Field::Hours, false), hms(23, 5, 5));
        assert_eq!(hms(0, 12, 0).step(Field::Minutes, true), hms(0, 13, 0));
    }

    #[test]
    fn shortcuts_carry_into_hours() {
        assert_eq!(hms(0, 55, 30).add_minutes(10), hms(1, 5, 30));
        assert_eq!(hms(0, 0, 0).add_minutes(1), hms(0, 1, 0));
    }

    #[test]
    fn shortcuts_clamp_at_the_maximum() {
        assert_eq!(hms(23, 55, 0).add_minutes(10), hms(23, 59, 59));
        assert_eq!(Hms::from_secs(u64::MAX).total_secs(), MAX_SECS);
    }

    #[test]
    fn zero_disables_start() {
        assert!(Hms::default().is_zero());
        assert!(!hms(0, 0, 1).is_zero());
        assert_eq!(hms(0, 12, 0).duration(), Duration::from_mins(12));
    }

    #[test]
    fn texts_are_two_digits() {
        assert_eq!(hms(0, 12, 5).texts(), ["00", "12", "05"]);
    }

    #[test]
    fn fields_map_from_slint_indexes() {
        assert_eq!(Field::from_index(0), Some(Field::Hours));
        assert_eq!(Field::from_index(2), Some(Field::Seconds));
        assert_eq!(Field::from_index(3), None);
    }
}
