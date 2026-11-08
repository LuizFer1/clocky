//! Dates in PT-BR for the reports ("dom, 8 de novembro", "2 a 8 de novembro"). chrono is built
//! without locales, so the names are here.

use chrono::{Datelike, NaiveDate, Weekday};

const WEEKDAYS: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];

const MONTHS: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// "seg" … "dom".
pub fn weekday_short(day: Weekday) -> &'static str {
    WEEKDAYS[day.num_days_from_monday() as usize]
}

/// "janeiro" … "dezembro".
pub fn month_name(date: NaiveDate) -> &'static str {
    MONTHS[date.month0() as usize]
}

/// "dom, 8 de novembro" (the year only when it is not `today`'s).
pub fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    format!(
        "{}, {}{}",
        weekday_short(date.weekday()),
        day_month(date),
        year_suffix(date, today)
    )
}

/// "2 a 8 de novembro", "27 de outubro a 2 de novembro" or, across years,
/// "29 de dezembro de 2025 a 4 de janeiro de 2026".
pub fn range_label(first: NaiveDate, last: NaiveDate, today: NaiveDate) -> String {
    if first.year() != last.year() {
        return format!(
            "{} de {} a {} de {}",
            day_month(first),
            first.year(),
            day_month(last),
            last.year()
        );
    }
    let start = if first.month() == last.month() {
        first.day().to_string()
    } else {
        day_month(first)
    };
    format!("{start} a {}{}", day_month(last), year_suffix(last, today))
}

/// "novembro de 2026".
pub fn month_label(date: NaiveDate) -> String {
    format!("{} de {}", month_name(date), date.year())
}

/// "8 de novembro".
fn day_month(date: NaiveDate) -> String {
    format!("{} de {}", date.day(), month_name(date))
}

fn year_suffix(date: NaiveDate, today: NaiveDate) -> String {
    if date.year() == today.year() {
        String::new()
    } else {
        format!(" de {}", date.year())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn weekday_and_month_names() {
        assert_eq!(weekday_short(Weekday::Mon), "seg");
        assert_eq!(weekday_short(Weekday::Sat), "sáb");
        assert_eq!(weekday_short(Weekday::Sun), "dom");
        assert_eq!(month_name(d(2026, 3, 1)), "março");
        assert_eq!(month_name(d(2026, 12, 31)), "dezembro");
    }

    #[test]
    fn day_label_adds_the_year_only_when_needed() {
        let today = d(2026, 11, 8);
        assert_eq!(day_label(today, today), "dom, 8 de novembro");
        assert_eq!(
            day_label(d(2025, 11, 3), today),
            "seg, 3 de novembro de 2025"
        );
    }

    #[test]
    fn range_label_names_the_month_once_when_it_can() {
        let today = d(2026, 11, 8);
        assert_eq!(
            range_label(d(2026, 11, 2), today, today),
            "2 a 8 de novembro"
        );
        assert_eq!(
            range_label(d(2026, 10, 27), d(2026, 11, 2), today),
            "27 de outubro a 2 de novembro"
        );
        assert_eq!(
            range_label(d(2025, 12, 29), d(2026, 1, 4), today),
            "29 de dezembro de 2025 a 4 de janeiro de 2026"
        );
        assert_eq!(
            range_label(d(2025, 3, 3), d(2025, 3, 9), today),
            "3 a 9 de março de 2025"
        );
    }

    #[test]
    fn month_label_has_the_year() {
        assert_eq!(month_label(d(2026, 11, 8)), "novembro de 2026");
    }
}
