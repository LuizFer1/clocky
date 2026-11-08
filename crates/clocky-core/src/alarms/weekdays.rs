//! Set of weekdays on which an alarm repeats.

use chrono::Weekday;

use super::error::AlarmError;

/// All seven weekdays, Monday first.
const ALL: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

/// Set of weekdays, stored as a bitmask (the `repeat_days` column).
///
/// Bit 0 is Monday, bit 1 Tuesday, and so on up to bit 6, Sunday. The empty set
/// (`0`) means the alarm rings once and does not repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Weekdays(u8);

impl Weekdays {
    /// No repetition: the alarm rings once.
    pub const ONCE: Self = Self(0);
    /// Monday to Friday.
    pub const WEEKDAYS: Self = Self(0b001_1111);
    /// Saturday and Sunday.
    pub const WEEKEND: Self = Self(0b110_0000);
    /// Every day of the week.
    pub const EVERY_DAY: Self = Self(0b111_1111);

    /// The set with only `day`.
    #[must_use]
    pub fn single(day: Weekday) -> Self {
        Self(bit(day))
    }

    /// This set plus `day`.
    #[must_use]
    pub fn with(self, day: Weekday) -> Self {
        Self(self.0 | bit(day))
    }

    /// Whether `day` is in the set.
    #[must_use]
    pub fn contains(self, day: Weekday) -> bool {
        self.0 & bit(day) != 0
    }

    /// Whether the set is empty, that is, the alarm rings once.
    #[must_use]
    pub fn is_once(self) -> bool {
        self.0 == 0
    }

    /// The days in the set, Monday first.
    pub fn iter(self) -> impl Iterator<Item = Weekday> {
        ALL.into_iter().filter(move |day| self.contains(*day))
    }

    /// The bitmask, as stored in the database.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

fn bit(day: Weekday) -> u8 {
    1 << day.num_days_from_monday()
}

impl FromIterator<Weekday> for Weekdays {
    fn from_iter<I: IntoIterator<Item = Weekday>>(days: I) -> Self {
        days.into_iter().fold(Self::ONCE, Self::with)
    }
}

impl TryFrom<u8> for Weekdays {
    type Error = AlarmError;

    fn try_from(bits: u8) -> Result<Self, Self::Error> {
        if bits & !Self::EVERY_DAY.0 == 0 {
            Ok(Self(bits))
        } else {
            Err(AlarmError::InvalidWeekdays(bits))
        }
    }
}

impl From<Weekdays> for u8 {
    fn from(days: Weekdays) -> Self {
        days.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_is_the_empty_set() {
        assert!(Weekdays::ONCE.is_once());
        assert_eq!(Weekdays::ONCE.iter().count(), 0);
        assert_eq!(u8::from(Weekdays::ONCE), 0);
    }

    #[test]
    fn monday_is_bit_zero_and_sunday_is_bit_six() {
        assert_eq!(Weekdays::single(Weekday::Mon).bits(), 0b000_0001);
        assert_eq!(Weekdays::single(Weekday::Sun).bits(), 0b100_0000);
    }

    #[test]
    fn weekdays_are_monday_to_friday() {
        let days: Vec<Weekday> = Weekdays::WEEKDAYS.iter().collect();
        assert_eq!(days, ALL[..5]);
        assert!(!Weekdays::WEEKDAYS.contains(Weekday::Sat));
    }

    #[test]
    fn weekend_is_saturday_and_sunday() {
        let days: Vec<Weekday> = Weekdays::WEEKEND.iter().collect();
        assert_eq!(days, [Weekday::Sat, Weekday::Sun]);
    }

    #[test]
    fn every_day_has_all_seven_days_monday_first() {
        let days: Vec<Weekday> = Weekdays::EVERY_DAY.iter().collect();
        assert_eq!(days, ALL);
    }

    #[test]
    fn single_day_contains_only_that_day() {
        let wed = Weekdays::single(Weekday::Wed);
        assert!(!wed.is_once());
        assert_eq!(wed.iter().collect::<Vec<_>>(), [Weekday::Wed]);
    }

    #[test]
    fn collecting_days_builds_the_set() {
        let set: Weekdays = [Weekday::Sat, Weekday::Sun, Weekday::Sat]
            .into_iter()
            .collect();
        assert_eq!(set, Weekdays::WEEKEND);
        assert_eq!(
            Weekdays::ONCE.with(Weekday::Fri),
            Weekdays::single(Weekday::Fri)
        );
    }

    #[test]
    fn every_valid_byte_roundtrips() {
        for bits in 0..=0x7F_u8 {
            let set = Weekdays::try_from(bits).unwrap();
            assert_eq!(u8::from(set), bits);
        }
    }

    #[test]
    fn bytes_above_sunday_are_rejected() {
        assert_eq!(
            Weekdays::try_from(0x80),
            Err(AlarmError::InvalidWeekdays(0x80))
        );
        assert_eq!(
            Weekdays::try_from(0xFF),
            Err(AlarmError::InvalidWeekdays(0xFF))
        );
    }

    #[test]
    fn invalid_bitmask_error_names_the_value() {
        let message = AlarmError::InvalidWeekdays(0x80).to_string();
        assert!(message.contains("0x80"), "{message}");
    }
}
