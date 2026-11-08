//! Alarm texts in the handoff's words (SPEC 2.1 and 2.5): the repeat summary ("seg–sex", "fins
//! de semana", "todos os dias" or a list), the editor's live summary, the Ativos card status and
//! the missed-alarm notice. Pure: no clock, the time zone is a parameter.

use std::path::Path;
use std::time::Duration;

use chrono::{Datelike, NaiveDateTime, TimeZone};
use clocky_core::alarms::{AlarmTime, Weekdays};
use clocky_core::store::Alarm;
use clocky_core::time::Timestamp;

/// Short day names, Monday first.
const SHORT_DAYS: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
/// Name of the built-in sound (the chime of `crate::sound`).
pub const DEFAULT_SOUND: &str = "Sino suave";
/// Name used when the alarm has none.
pub const DEFAULT_NAME: &str = "Alarme";

/// The days of a repeating alarm: "todos os dias", "seg–sex", "fins de semana" or the list
/// ("seg, qua, sex"). Empty for a once-alarm.
pub fn days_summary(days: Weekdays) -> String {
    match days {
        Weekdays::EVERY_DAY => "todos os dias".to_owned(),
        Weekdays::WEEKDAYS => "seg–sex".to_owned(),
        Weekdays::WEEKEND => "fins de semana".to_owned(),
        _ => days
            .iter()
            .map(|day| SHORT_DAYS[day.num_days_from_monday() as usize])
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// How an alarm repeats, for cards and links: the days, or "uma vez".
pub fn repeat_label(repeat: Weekdays) -> String {
    if repeat.is_once() {
        "uma vez".to_owned()
    } else {
        days_summary(repeat)
    }
}

/// The editor's live summary. `repeat` is `None` for "Uma vez"; `Some` with no day selected
/// asks for one (the alarm cannot be saved like that).
pub fn ring_summary(time: AlarmTime, repeat: Option<Weekdays>) -> String {
    match repeat {
        None => format!("Toca uma vez, às {time}"),
        Some(days) if days.is_once() => "Escolha ao menos um dia".to_owned(),
        Some(days) => format!("Toca {} às {time}", days_summary(days)),
    }
}

/// Local date and time of `at` in `tz` (`None` in a DST gap, which an instant never falls in).
fn local<Tz: TimeZone>(at: Timestamp, tz: &Tz) -> Option<NaiveDateTime> {
    tz.timestamp_millis_opt(at.as_millis())
        .earliest()
        .map(|t| t.naive_local())
}

/// `HH:MM` of `at` in `tz`.
pub fn hh_mm<Tz: TimeZone>(at: Timestamp, tz: &Tz) -> String {
    local(at, tz).map_or_else(|| "--:--".to_owned(), |t| t.format("%H:%M").to_string())
}

/// The day of `due` seen from `now`: "hoje", "amanhã", the weekday within a week ("qua"), else
/// "dd/mm".
pub fn day_label<Tz: TimeZone>(due: Timestamp, now: Timestamp, tz: &Tz) -> String {
    let (Some(due), Some(now)) = (local(due, tz), local(now, tz)) else {
        return String::new();
    };
    match (due.date() - now.date()).num_days() {
        0 => "hoje".to_owned(),
        1 => "amanhã".to_owned(),
        2..=6 => SHORT_DAYS[due.weekday().num_days_from_monday() as usize].to_owned(),
        _ => format!("{:02}/{:02}", due.day(), due.month()),
    }
}

/// Status line of an alarm card in Ativos (not ringing): "seg–sex · amanhã", "uma vez · hoje",
/// "soneca até 07:35" or "seg–sex · desligado".
pub fn card_status<Tz: TimeZone>(alarm: &Alarm, now: Timestamp, tz: &Tz) -> String {
    let repeat = repeat_label(alarm.repeat);
    if !alarm.enabled {
        return format!("{repeat} · desligado");
    }
    if let Some(until) = alarm.snoozed_until {
        return format!("soneca até {}", hh_mm(until, tz));
    }
    match alarm.schedule().due_at(now, tz) {
        Some(due) => format!("{repeat} · {}", day_label(due, now, tz)),
        None => repeat,
    }
}

/// The in-app notice of a missed alarm (spec, decision 4): "Alarme perdido às 07:30 · nome",
/// with the date when it was not today ("Alarme perdido em 05/11 às 07:30 · nome").
pub fn missed_notice<Tz: TimeZone>(name: &str, due: Timestamp, now: Timestamp, tz: &Tz) -> String {
    let time = hh_mm(due, tz);
    let same_day =
        matches!((local(due, tz), local(now, tz)), (Some(d), Some(n)) if d.date() == n.date());
    let when = match local(due, tz) {
        Some(d) if !same_day => format!("em {:02}/{:02} às {time}", d.day(), d.month()),
        _ => format!("às {time}"),
    };
    format!("Alarme perdido {when} · {}", display_name(name))
}

/// How late a late alarm rings: "atrasado 12 min" (at least 1 min).
pub fn late_label(by: Duration) -> String {
    format!("atrasado {} min", (by.as_secs() / 60).max(1))
}

/// The name shown for an alarm: its own, or "Alarme".
pub fn display_name(name: &str) -> &str {
    let name = name.trim();
    if name.is_empty() { DEFAULT_NAME } else { name }
}

/// The sound field: "Sino suave" for the built-in chime, else the file name.
pub fn sound_label(path: Option<&str>) -> String {
    match path.map(str::trim).filter(|p| !p.is_empty()) {
        None => DEFAULT_SOUND.to_owned(),
        Some(path) => Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Utc, Weekday};
    use clocky_core::domain::AlarmId;

    use super::*;

    fn time(h: u8, m: u8) -> AlarmTime {
        AlarmTime::new(h, m).unwrap()
    }

    /// 2026-11-08 (a Sunday) at `h:m` UTC, plus `days`.
    fn at(days: i64, h: u32, m: u32) -> Timestamp {
        let base = Utc.with_ymd_and_hms(2026, 11, 8, h, m, 0).unwrap();
        Timestamp::from_millis(base.timestamp_millis() + days * 86_400_000)
    }

    fn alarm(repeat: Weekdays) -> Alarm {
        Alarm {
            id: AlarmId(1),
            name: "Daily standup".into(),
            time: time(7, 30),
            repeat,
            sound: None,
            message: String::new(),
            snooze_minutes: 5,
            task: None,
            enabled: true,
            snoozed_until: None,
            last_fired_at: None,
            created_at: at(-1, 0, 0),
        }
    }

    #[test]
    fn named_day_sets_use_the_handoff_words() {
        assert_eq!(days_summary(Weekdays::WEEKDAYS), "seg–sex");
        assert_eq!(days_summary(Weekdays::WEEKEND), "fins de semana");
        assert_eq!(days_summary(Weekdays::EVERY_DAY), "todos os dias");
    }

    #[test]
    fn other_day_sets_are_listed_monday_first() {
        let days: Weekdays = [Weekday::Fri, Weekday::Mon, Weekday::Wed]
            .into_iter()
            .collect();
        assert_eq!(days_summary(days), "seg, qua, sex");
        assert_eq!(days_summary(Weekdays::single(Weekday::Sat)), "sáb");
        assert_eq!(repeat_label(Weekdays::ONCE), "uma vez");
        assert_eq!(repeat_label(Weekdays::WEEKEND), "fins de semana");
    }

    #[test]
    fn ring_summary_follows_screens_10_and_11() {
        assert_eq!(
            ring_summary(time(7, 30), Some(Weekdays::WEEKDAYS)),
            "Toca seg–sex às 07:30"
        );
        assert_eq!(ring_summary(time(7, 30), None), "Toca uma vez, às 07:30");
        assert_eq!(
            ring_summary(time(7, 30), Some(Weekdays::ONCE)),
            "Escolha ao menos um dia"
        );
    }

    #[test]
    fn day_label_counts_local_days() {
        let now = at(0, 22, 0);
        assert_eq!(day_label(at(0, 23, 0), now, &Utc), "hoje");
        assert_eq!(day_label(at(1, 7, 30), now, &Utc), "amanhã");
        // 2026-11-11 is a Wednesday.
        assert_eq!(day_label(at(3, 7, 30), now, &Utc), "qua");
        assert_eq!(day_label(at(9, 7, 30), now, &Utc), "17/11");
    }

    #[test]
    fn card_status_names_the_next_day_snooze_or_off() {
        let now = at(0, 22, 0);
        // Sunday night: the next weekday ring is Monday morning.
        assert_eq!(
            card_status(&alarm(Weekdays::WEEKDAYS), now, &Utc),
            "seg–sex · amanhã"
        );
        assert_eq!(
            card_status(&alarm(Weekdays::ONCE), at(0, 6, 0), &Utc),
            "uma vez · hoje"
        );
        let snoozed = Alarm {
            snoozed_until: Some(at(0, 7, 35)),
            ..alarm(Weekdays::WEEKDAYS)
        };
        assert_eq!(
            card_status(&snoozed, at(0, 7, 31), &Utc),
            "soneca até 07:35"
        );
        let off = Alarm {
            enabled: false,
            ..alarm(Weekdays::WEEKEND)
        };
        assert_eq!(card_status(&off, now, &Utc), "fins de semana · desligado");
    }

    #[test]
    fn missed_notice_says_the_time_and_the_day_when_not_today() {
        assert_eq!(
            missed_notice("Daily standup", at(0, 7, 30), at(0, 9, 0), &Utc),
            "Alarme perdido às 07:30 · Daily standup"
        );
        assert_eq!(
            missed_notice("  ", at(-3, 7, 30), at(0, 9, 0), &Utc),
            "Alarme perdido em 05/11 às 07:30 · Alarme"
        );
    }

    #[test]
    fn late_label_rounds_down_to_whole_minutes() {
        assert_eq!(
            late_label(Duration::from_secs(12 * 60 + 40)),
            "atrasado 12 min"
        );
        assert_eq!(late_label(Duration::from_secs(61)), "atrasado 1 min");
    }

    #[test]
    fn sound_label_is_the_chime_or_the_file_name() {
        assert_eq!(sound_label(None), "Sino suave");
        assert_eq!(sound_label(Some(" ")), "Sino suave");
        assert_eq!(sound_label(Some("C:/sons/galo.mp3")), "galo.mp3");
    }

    #[test]
    fn hh_mm_formats_in_the_given_zone() {
        assert_eq!(hh_mm(at(0, 7, 5), &Utc), "07:05");
        let tz = chrono::FixedOffset::west_opt(3 * 3600).unwrap();
        assert_eq!(hh_mm(at(0, 10, 30), &tz), "07:30");
    }
}
