//! Local time of day at which an alarm rings.

use std::fmt;
use std::str::FromStr;

use chrono::{NaiveTime, TimeDelta};

use super::error::AlarmError;

/// Local wall-clock time `HH:MM` (the `time_local` column).
///
/// It has no date and no time zone: the zone is applied when the next ring is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AlarmTime {
    hour: u8,
    minute: u8,
}

impl AlarmTime {
    /// Creates the time `hour:minute`.
    ///
    /// # Errors
    /// [`AlarmError::InvalidTime`] if `hour > 23` or `minute > 59`.
    pub fn new(hour: u8, minute: u8) -> Result<Self, AlarmError> {
        if hour < 24 && minute < 60 {
            Ok(Self { hour, minute })
        } else {
            Err(AlarmError::InvalidTime(format!("{hour:02}:{minute:02}")))
        }
    }

    /// Hour, from 0 to 23.
    #[must_use]
    pub const fn hour(self) -> u8 {
        self.hour
    }

    /// Minute, from 0 to 59.
    #[must_use]
    pub const fn minute(self) -> u8 {
        self.minute
    }

    /// The same time as a [`NaiveTime`], with zero seconds.
    #[must_use]
    pub fn to_naive_time(self) -> NaiveTime {
        let minutes = i64::from(self.hour) * 60 + i64::from(self.minute);
        NaiveTime::MIN + TimeDelta::minutes(minutes)
    }
}

impl FromStr for AlarmTime {
    type Err = AlarmError;

    /// Parses exactly `HH:MM`, with two digits on each side (the database format).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || AlarmError::InvalidTime(text.to_owned());
        let (hour, minute) = text.split_once(':').ok_or_else(invalid)?;
        let hour = two_digits(hour).ok_or_else(invalid)?;
        let minute = two_digits(minute).ok_or_else(invalid)?;
        Self::new(hour, minute).map_err(|_| invalid())
    }
}

/// Parses exactly two ASCII digits.
fn two_digits(text: &str) -> Option<u8> {
    let is_two_digits = text.len() == 2 && text.bytes().all(|b| b.is_ascii_digit());
    is_two_digits.then(|| text.parse().ok()).flatten()
}

impl fmt::Display for AlarmTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(hour: u8, minute: u8) -> AlarmTime {
        AlarmTime::new(hour, minute).unwrap()
    }

    #[test]
    fn new_accepts_the_whole_day() {
        assert_eq!(time(0, 0).hour(), 0);
        assert_eq!(time(23, 59).hour(), 23);
        assert_eq!(time(23, 59).minute(), 59);
    }

    #[test]
    fn new_rejects_out_of_range_values() {
        assert!(matches!(
            AlarmTime::new(24, 0),
            Err(AlarmError::InvalidTime(_))
        ));
        assert!(matches!(
            AlarmTime::new(7, 60),
            Err(AlarmError::InvalidTime(_))
        ));
    }

    #[test]
    fn displays_as_zero_padded_hh_mm() {
        assert_eq!(time(7, 5).to_string(), "07:05");
        assert_eq!(time(23, 59).to_string(), "23:59");
        assert_eq!(time(0, 0).to_string(), "00:00");
    }

    #[test]
    fn parses_hh_mm() {
        assert_eq!("07:05".parse::<AlarmTime>(), Ok(time(7, 5)));
        assert_eq!("23:59".parse::<AlarmTime>(), Ok(time(23, 59)));
        assert_eq!("00:00".parse::<AlarmTime>(), Ok(time(0, 0)));
    }

    #[test]
    fn every_minute_of_the_day_roundtrips_through_text() {
        for hour in 0..24 {
            for minute in 0..60 {
                let t = time(hour, minute);
                assert_eq!(t.to_string().parse::<AlarmTime>(), Ok(t));
            }
        }
    }

    #[test]
    fn rejects_malformed_text() {
        for bad in [
            "",
            "7:05",
            "07:5",
            "0705",
            "07-05",
            "07:05:00",
            "24:00",
            "07:60",
            "ab:cd",
            "+7:05",
            " 07:05",
            "07:05 ",
            "٠٧:٠٥",
        ] {
            assert_eq!(
                bad.parse::<AlarmTime>(),
                Err(AlarmError::InvalidTime(bad.to_owned())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn converts_to_naive_time_with_zero_seconds() {
        let expected = NaiveTime::from_hms_opt(7, 5, 0).unwrap();
        assert_eq!(time(7, 5).to_naive_time(), expected);
    }

    #[test]
    fn invalid_time_error_quotes_the_input() {
        let message = AlarmError::InvalidTime("25:00".to_owned()).to_string();
        assert!(message.contains("\"25:00\""), "{message}");
    }
}
