//! The alarm being edited in the Alarme panel (SPEC 2.1): time, repetition, name, task, sound,
//! snooze and notification text, with the editing rules. Pure: the panel pushes it to Slint.

use chrono::{TimeZone, Timelike};
use clocky_core::alarms::{AlarmTime, Weekdays};
use clocky_core::domain::{AlarmId, TaskId};
use clocky_core::store::{Alarm, NewAlarm};
use clocky_core::time::Timestamp;

use super::summary;

/// Snooze limits of the stepper (minutes).
pub const SNOOZE_MIN: u32 = 1;
pub const SNOOZE_MAX: u32 = 30;
/// Snooze of a new alarm (minutes).
pub const SNOOZE_DEFAULT: u32 = 5;
/// Summary while a typed field is not a valid time.
pub const INVALID_TIME: &str = "Horário inválido: use 00–23 e 00–59";
/// The minute arrows move by this much, snapping to its multiples.
pub const MINUTE_STEP: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The alarm being edited; `None` = a new one.
    pub editing: Option<AlarmId>,
    pub hour: u8,
    pub minute: u8,
    /// "Dias da semana" (`true`) or "Uma vez".
    pub repeating: bool,
    /// The selected chips. Kept while "Uma vez" is chosen, so switching back restores them.
    pub days: Weekdays,
    pub name: String,
    pub task: Option<TaskId>,
    /// Custom sound file; `None` = the built-in chime.
    pub sound: Option<String>,
    pub snooze: u32,
    pub message: String,
    /// The typed hours or minutes are not a valid time: Salvar waits until they are fixed (the
    /// arrows or any other change put the draft's value back in the fields).
    pub bad_hour: bool,
    pub bad_minute: bool,
}

impl Draft {
    /// A new alarm at `hour:minute` that rings once (the chips start on seg–sex).
    pub fn new(hour: u8, minute: u8) -> Self {
        Self {
            editing: None,
            hour: hour.min(23),
            minute: minute.min(59),
            repeating: false,
            days: Weekdays::WEEKDAYS,
            name: String::new(),
            task: None,
            sound: None,
            snooze: SNOOZE_DEFAULT,
            message: String::new(),
            bad_hour: false,
            bad_minute: false,
        }
    }

    /// A new alarm at the next whole local hour after `now`.
    pub fn at_next_hour<Tz: TimeZone>(now: Timestamp, tz: &Tz) -> Self {
        let hour = tz
            .timestamp_millis_opt(now.as_millis())
            .earliest()
            .map_or(7, |t| (t.hour() + 1) % 24);
        Self::new(u8::try_from(hour).unwrap_or(7), 0)
    }

    /// The saved alarm, ready to edit.
    pub fn from_alarm(alarm: &Alarm) -> Self {
        Self {
            editing: Some(alarm.id),
            hour: alarm.time.hour(),
            minute: alarm.time.minute(),
            repeating: !alarm.repeat.is_once(),
            days: if alarm.repeat.is_once() {
                Weekdays::WEEKDAYS
            } else {
                alarm.repeat
            },
            name: alarm.name.clone(),
            task: alarm.task,
            sound: alarm.sound.clone(),
            snooze: alarm.snooze_minutes.clamp(SNOOZE_MIN, SNOOZE_MAX),
            message: alarm.message.clone(),
            bad_hour: false,
            bad_minute: false,
        }
    }

    /// The panel title.
    pub fn title(&self) -> &'static str {
        if self.editing.is_some() {
            "Editar alarme"
        } else {
            "Novo alarme"
        }
    }

    /// The time of day (the fields always hold a valid one; wrapped just in case).
    pub fn time(&self) -> Option<AlarmTime> {
        AlarmTime::new(self.hour % 24, self.minute % 60).ok()
    }

    /// ↑/↓ of the hours, wrapping around midnight.
    pub fn step_hour(&mut self, up: bool) {
        self.hour = if up {
            (self.hour + 1) % 24
        } else {
            (self.hour + 23) % 24
        };
    }

    /// ↑/↓ of the minutes: to the next or previous multiple of 5, wrapping inside the hour.
    pub fn step_minute(&mut self, up: bool) {
        let m = self.minute;
        self.minute = if up {
            (m / MINUTE_STEP + 1) * MINUTE_STEP % 60
        } else if !m.is_multiple_of(MINUTE_STEP) {
            m - m % MINUTE_STEP
        } else {
            (m + 60 - MINUTE_STEP) % 60
        };
    }

    /// Typed hours: applied when it is a number from 0 to 23. Returns whether it was.
    pub fn set_hour_text(&mut self, text: &str) -> bool {
        let ok = parse_in(text, 23).map(|h| self.hour = h).is_some();
        self.bad_hour = !ok;
        ok
    }

    /// Typed minutes: applied when it is a number from 0 to 59. Returns whether it was.
    pub fn set_minute_text(&mut self, text: &str) -> bool {
        let ok = parse_in(text, 59).map(|m| self.minute = m).is_some();
        self.bad_minute = !ok;
        ok
    }

    /// Turns the chip of day `index` (0 = Monday) on or off.
    pub fn toggle_day(&mut self, index: usize) {
        let Some(bit) = u32::try_from(index).ok().filter(|i| *i < 7) else {
            return;
        };
        let bits = self.days.bits() ^ (1 << bit);
        if let Ok(days) = Weekdays::try_from(bits) {
            self.days = days;
        }
    }

    /// Whether chip `index` (0 = Monday) is on.
    pub fn day_on(&self, index: usize) -> bool {
        u32::try_from(index)
            .ok()
            .filter(|i| *i < 7)
            .is_some_and(|i| self.days.bits() & (1 << i) != 0)
    }

    /// −/+ of the snooze stepper, from 1 to 30 minutes.
    pub fn bump_snooze(&mut self, up: bool) {
        let next = if up {
            self.snooze.saturating_add(1)
        } else {
            self.snooze.saturating_sub(1)
        };
        self.snooze = next.clamp(SNOOZE_MIN, SNOOZE_MAX);
    }

    /// The repetition as the summary sees it: `None` for "Uma vez".
    pub fn repeat(&self) -> Option<Weekdays> {
        self.repeating.then_some(self.days)
    }

    /// "Dias da semana" needs at least one day (an empty set would ring once).
    /// A typed field holds something that is not a valid hour or minute.
    pub fn bad_time(&self) -> bool {
        self.bad_hour || self.bad_minute
    }

    pub fn can_save(&self) -> bool {
        !self.bad_time() && (!self.repeating || !self.days.is_once())
    }

    pub fn summary(&self) -> String {
        if self.bad_time() {
            return INVALID_TIME.to_owned();
        }
        self.time()
            .map(|time| summary::ring_summary(time, self.repeat()))
            .unwrap_or_default()
    }

    /// The fields to save; `None` while it cannot be saved.
    pub fn to_new(&self) -> Option<NewAlarm<'_>> {
        let time = self.time().filter(|_| self.can_save())?;
        Some(NewAlarm {
            name: summary::display_name(&self.name),
            time,
            repeat: self.repeat().unwrap_or(Weekdays::ONCE),
            sound: self
                .sound
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            message: self.message.trim(),
            snooze_minutes: self.snooze.clamp(SNOOZE_MIN, SNOOZE_MAX),
            task: self.task,
        })
    }
}

/// Whether saving `new` over `old` arms the alarm again: only a once-alarm that already rang
/// (switched off after it fired, or snoozed) whose time the user changed. Every other save keeps
/// the enabled state (a repeating alarm switched off stays off when only its name changes).
pub fn rearms_on_save(old: &Alarm, new: &NewAlarm<'_>) -> bool {
    let rang = !old.enabled || old.snoozed_until.is_some();
    new.repeat.is_once() && old.time != new.time && rang
}

/// A number of one or two digits from 0 to `max`.
fn parse_in(text: &str, max: u8) -> Option<u8> {
    let text = text.trim();
    let digits = (1..=2).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
    digits
        .then(|| text.parse::<u8>().ok())
        .flatten()
        .filter(|v| *v <= max)
}

/// Two-digit text of an hour or minute field.
pub fn two_digits(value: u8) -> String {
    format!("{value:02}")
}

#[cfg(test)]
mod tests {
    use chrono::{Utc, Weekday};

    use super::*;

    #[test]
    fn hours_wrap_around_midnight() {
        let mut d = Draft::new(23, 0);
        d.step_hour(true);
        assert_eq!(d.hour, 0);
        d.step_hour(false);
        assert_eq!(d.hour, 23);
    }

    #[test]
    fn minutes_move_by_five_snapping_and_wrap_inside_the_hour() {
        let mut d = Draft::new(7, 32);
        d.step_minute(true);
        assert_eq!(d.minute, 35);
        d.minute = 32;
        d.step_minute(false);
        assert_eq!(d.minute, 30);
        d.step_minute(false);
        assert_eq!(d.minute, 25);
        d.minute = 55;
        d.step_minute(true);
        assert_eq!((d.hour, d.minute), (7, 0));
        d.step_minute(false);
        assert_eq!((d.hour, d.minute), (7, 55));
    }

    #[test]
    fn typed_fields_accept_only_valid_numbers() {
        let mut d = Draft::new(7, 30);
        assert!(d.set_hour_text("9"));
        assert!(d.set_minute_text("07"));
        assert_eq!(d.time().unwrap().to_string(), "09:07");
        assert!(!d.set_hour_text("24"));
        assert!(!d.set_minute_text("60"));
        assert!(!d.set_minute_text("1a"));
        assert!(!d.set_hour_text(""));
        assert!(!d.set_hour_text("007"));
        assert_eq!(d.time().unwrap().to_string(), "09:07");
    }

    #[test]
    fn a_bad_typed_time_blocks_saving_until_both_fields_are_valid() {
        let mut d = Draft::new(7, 30);
        for bad in ["", "24", "7a", "123"] {
            assert!(!d.set_hour_text(bad));
            assert!(!d.can_save() && d.to_new().is_none());
            assert_eq!(d.summary(), INVALID_TIME);
        }
        assert!(!d.set_minute_text("60"));
        assert!(d.set_hour_text("8"));
        assert!(!d.can_save(), "the minutes are still bad");
        assert!(d.set_minute_text("05"));
        assert!(d.can_save());
        assert_eq!(d.to_new().unwrap().time.to_string(), "08:05");
    }

    #[test]
    fn chips_toggle_and_survive_switching_to_once() {
        let mut d = Draft::new(7, 30);
        d.repeating = true;
        d.toggle_day(4);
        assert!(!d.day_on(4));
        assert_eq!(d.summary(), "Toca seg, ter, qua, qui às 07:30");
        d.repeating = false;
        assert_eq!(d.summary(), "Toca uma vez, às 07:30");
        d.repeating = true;
        assert!(d.day_on(0) && !d.day_on(4));
        d.toggle_day(7);
        assert!(!d.day_on(7));
    }

    #[test]
    fn weekdays_with_no_day_cannot_be_saved() {
        let mut d = Draft::new(7, 30);
        d.repeating = true;
        for day in 0..5 {
            d.toggle_day(day);
        }
        assert!(!d.can_save());
        assert!(d.to_new().is_none());
        assert_eq!(d.summary(), "Escolha ao menos um dia");
        d.repeating = false;
        assert_eq!(d.to_new().unwrap().repeat, Weekdays::ONCE);
    }

    #[test]
    fn snooze_stays_between_1_and_30() {
        let mut d = Draft::new(7, 30);
        d.snooze = 1;
        d.bump_snooze(false);
        assert_eq!(d.snooze, 1);
        d.snooze = 30;
        d.bump_snooze(true);
        assert_eq!(d.snooze, 30);
        d.bump_snooze(false);
        assert_eq!(d.snooze, 29);
    }

    #[test]
    fn new_alarm_fields_trim_and_default_the_name() {
        let mut d = Draft::new(6, 0);
        d.repeating = true;
        d.days = Weekdays::WEEKEND;
        d.message = "  Hora!  ".into();
        d.sound = Some("  ".into());
        let new = d.to_new().unwrap();
        assert_eq!(new.name, "Alarme");
        assert_eq!(new.message, "Hora!");
        assert_eq!(new.sound, None);
        assert_eq!(new.repeat, Weekdays::WEEKEND);
        assert_eq!(new.snooze_minutes, 5);
        assert_eq!(d.title(), "Novo alarme");
    }

    #[test]
    fn a_saved_alarm_loads_into_the_editor() {
        let alarm = Alarm {
            id: AlarmId(3),
            name: "Academia".into(),
            time: AlarmTime::new(18, 15).unwrap(),
            repeat: Weekdays::single(Weekday::Wed),
            sound: Some("C:/galo.wav".into()),
            message: "Bora".into(),
            snooze_minutes: 10,
            task: Some(TaskId(4)),
            enabled: true,
            snoozed_until: None,
            last_fired_at: None,
            created_at: Timestamp::from_millis(0),
        };
        let d = Draft::from_alarm(&alarm);
        assert_eq!(d.title(), "Editar alarme");
        assert_eq!((d.hour, d.minute, d.repeating), (18, 15, true));
        assert!(d.day_on(2) && !d.day_on(0));
        assert_eq!(d.to_new().unwrap().sound, Some("C:/galo.wav"));
        let once = Draft::from_alarm(&Alarm {
            repeat: Weekdays::ONCE,
            ..alarm
        });
        assert!(!once.repeating);
        assert_eq!(once.days, Weekdays::WEEKDAYS);
    }

    #[test]
    fn only_a_rung_once_alarm_with_a_new_time_is_armed_again() {
        let old = Alarm {
            id: AlarmId(1),
            name: "x".into(),
            time: AlarmTime::new(7, 30).unwrap(),
            repeat: Weekdays::ONCE,
            sound: None,
            message: String::new(),
            snooze_minutes: 5,
            task: None,
            enabled: false,
            snoozed_until: None,
            last_fired_at: None,
            created_at: Timestamp::from_millis(0),
        };
        let mut d = Draft::from_alarm(&old);
        assert!(!rearms_on_save(&old, &d.to_new().unwrap()), "same time");
        d.step_hour(true);
        assert!(rearms_on_save(&old, &d.to_new().unwrap()));
        let snoozed = Alarm {
            enabled: true,
            snoozed_until: Some(Timestamp::from_millis(9)),
            ..old.clone()
        };
        assert!(rearms_on_save(&snoozed, &d.to_new().unwrap()));
        let on = Alarm {
            enabled: true,
            ..old.clone()
        };
        assert!(!rearms_on_save(&on, &d.to_new().unwrap()), "already on");
        let weekly_off = Alarm {
            repeat: Weekdays::WEEKDAYS,
            ..old
        };
        let mut w = Draft::from_alarm(&weekly_off);
        w.step_hour(true);
        assert!(
            !rearms_on_save(&weekly_off, &w.to_new().unwrap()),
            "repeating"
        );
    }

    #[test]
    fn new_alarms_start_at_the_next_whole_hour() {
        let now = Utc.with_ymd_and_hms(2026, 11, 8, 14, 20, 0).unwrap();
        let d = Draft::at_next_hour(Timestamp::from_millis(now.timestamp_millis()), &Utc);
        assert_eq!((d.hour, d.minute, d.repeating), (15, 0, false));
        let late = Utc.with_ymd_and_hms(2026, 11, 8, 23, 40, 0).unwrap();
        let d = Draft::at_next_hour(Timestamp::from_millis(late.timestamp_millis()), &Utc);
        assert_eq!(d.hour, 0);
        assert_eq!(two_digits(7), "07");
    }
}
