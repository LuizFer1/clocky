use chrono::FixedOffset;
use clocky_core::clock::FakeClock;
use slint::Model;

use super::*;
use crate::demo;

/// 2026-11-08 (a Sunday) 20:00 at UTC-3.
const NOW: i64 = 1_794_178_800_000;

fn tz() -> FixedOffset {
    FixedOffset::west_opt(3 * 3600).unwrap()
}

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

fn today() -> NaiveDate {
    d(11, 8)
}

fn demo_core() -> AppCore {
    let mut core = AppCore::in_memory(Box::new(FakeClock::new(Timestamp::from_millis(NOW))));
    demo::seed_if_empty(&mut core, &tz()).unwrap();
    core
}

fn nav(tab: Tab, date: NaiveDate) -> ReportNav {
    ReportNav {
        tab,
        ..ReportNav::new(date)
    }
}

fn view(core: &AppCore, nav: &ReportNav) -> ReportsView {
    build(core, nav, today(), &tz()).unwrap()
}

#[test]
fn starts_on_this_week() {
    let nav = ReportNav::new(today());
    assert_eq!((nav.tab, nav.unit()), (Tab::Weekly, Range::Week));
    assert_eq!(nav.bounds(), (d(11, 2), d(11, 8)));
    assert!(nav.at_current(today()));
    assert_eq!(nav.current_label(), "Esta semana");
    assert_eq!(nav.period_label(today()), "2 a 8 de novembro");
}

#[test]
fn arrows_move_by_the_unit_of_the_tab() {
    let mut daily = nav(Tab::Daily, today());
    daily.shift(false);
    assert_eq!(daily.date, d(11, 7));
    assert!(!daily.at_current(today()));
    assert_eq!(daily.current_label(), "Hoje");
    assert_eq!(daily.period_label(today()), "sáb, 7 de novembro");
    let mut weekly = nav(Tab::Weekly, today());
    weekly.shift(false);
    assert_eq!(weekly.bounds(), (d(10, 26), d(11, 1)));
    assert_eq!(
        weekly.period_label(today()),
        "26 de outubro a 1 de novembro"
    );
    weekly.shift(true);
    assert!(weekly.at_current(today()));
    let mut month = ReportNav {
        tab: Tab::Overview,
        range: Range::Month,
        ..ReportNav::new(d(3, 31))
    };
    month.shift(false);
    assert_eq!(month.date, d(2, 28));
    assert_eq!(month.bounds(), (d(2, 1), d(2, 28)));
    assert_eq!(month.current_label(), "Este mês");
    assert_eq!(month.period_label(today()), "fevereiro de 2026");
}

#[test]
fn the_period_runs_from_the_first_midnight_to_the_last() {
    let week = ReportNav::new(today()).period(&tz()).unwrap();
    assert_eq!(week, Period::week_of(today(), &tz()).unwrap());
    let day = nav(Tab::Daily, today()).period(&tz()).unwrap();
    assert_eq!(day, Period::day(today(), &tz()).unwrap());
    let month = ReportNav {
        tab: Tab::Overview,
        range: Range::Month,
        ..ReportNav::new(today())
    };
    let period = month.period(&tz()).unwrap();
    assert_eq!(
        period.start(),
        Period::day(d(11, 1), &tz()).unwrap().start()
    );
    assert_eq!(period.end(), Period::day(d(12, 1), &tz()).unwrap().start());
}

#[test]
fn tabs_and_ranges_come_from_their_indexes() {
    assert_eq!(Tab::from_index(0), Tab::Overview);
    assert_eq!(Tab::from_index(2), Tab::Daily);
    assert_eq!(Tab::from_index(9), Tab::Weekly);
    assert_eq!(Range::from_index(0), Range::Day);
    assert_eq!(Range::from_index(2), Range::Month);
    assert_eq!(Range::from_index(-1), Range::Week);
}

#[test]
fn overview_lists_projects_then_tasks_with_bars() {
    let core = demo_core();
    let nav = ReportNav {
        tab: Tab::Overview,
        ..ReportNav::new(today())
    };
    let view = view(&core, &nav);
    assert!(!view.empty);
    assert_eq!(view.total, "15h 25min");
    assert_eq!(view.crumb, "Relatórios / Geral");
    assert_eq!(view.subheading, "2 a 8 de novembro · 15h 25min registrados");
    let Content::Overview(rows) = view.content else {
        panic!("not the overview")
    };
    let first = &rows[0];
    assert_eq!(
        (
            first.name.as_str(),
            first.total.as_str(),
            first.share.as_str()
        ),
        ("faculdade", "6h 40min", "43%")
    );
    assert!(!first.is_task && (first.bar - 1.0).abs() < f32::EPSILON);
    assert_eq!(first.color, palette::color("#9b7be0"));
    let tcc = &rows[1];
    assert!(tcc.is_task);
    assert_eq!((tcc.name.as_str(), tcc.total.as_str()), ("TCC", "5h 10min"));
    assert!((tcc.bar - 310.0 / 400.0).abs() < 1e-6);
    let names: Vec<_> = rows
        .iter()
        .filter(|r| !r.is_task)
        .map(|r| r.name.to_string())
        .collect();
    assert_eq!(names, ["faculdade", "connect", "pessoal"]);
}

#[test]
fn time_without_a_task_is_under_sem_projeto() {
    let mut core = demo_core();
    let auth = core.search_tasks("autenticação").unwrap()[0].id;
    core.delete_task(auth).unwrap();
    let nav = ReportNav {
        tab: Tab::Overview,
        range: Range::Day,
        ..ReportNav::new(today())
    };
    let Content::Overview(rows) = view(&core, &nav).content else {
        panic!("not the overview")
    };
    assert_eq!(rows.len(), 2);
    assert!(rows[0].none && !rows[0].is_task);
    assert_eq!(rows[0].name, NO_PROJECT);
    assert!(rows[1].none && rows[1].is_task);
    assert_eq!(
        (rows[1].name.as_str(), rows[1].total.as_str()),
        (NO_TASK, "1h 50min")
    );
}

#[test]
fn weekly_stacks_each_day_on_one_scale() {
    let core = demo_core();
    let view = view(&core, &ReportNav::new(today()));
    assert_eq!(view.crumb, "Relatórios / Semanal");
    let Content::Weekly(week) = view.content else {
        panic!("not the week")
    };
    let labels: Vec<_> = week
        .days
        .iter()
        .map(|c| format!("{} {}", c.weekday, c.day))
        .collect();
    assert_eq!(
        labels,
        [
            "seg 2", "ter 3", "qua 4", "qui 5", "sex 6", "sáb 7", "dom 8"
        ]
    );
    assert_eq!(
        (week.scale_top.as_str(), week.scale_mid.as_str()),
        ("4h", "2h")
    );
    let thursday = &week.days[3];
    assert_eq!(thursday.total, "3h 15min");
    assert!((thursday.height - 195.0 / 240.0).abs() < 1e-6);
    // Top to bottom: pessoal (135 min) over connect (60 min), the week's order reversed.
    let parts: Vec<_> = thursday.parts.iter().collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].color, palette::color("#3fae6a"));
    assert!((parts[0].size - 135.0 / 195.0).abs() < 1e-6);
    assert!(week.days[6].today && !week.days[5].today);
    let legend: Vec<_> = week
        .projects
        .iter()
        .map(|p| (p.name.to_string(), p.total.to_string()))
        .collect();
    assert_eq!(
        legend,
        [
            ("faculdade".to_owned(), "6h 40min".to_owned()),
            ("connect".to_owned(), "5h 30min".to_owned()),
            ("pessoal".to_owned(), "3h 15min".to_owned()),
        ]
    );
}

#[test]
fn an_empty_week_says_so() {
    let core = demo_core();
    let view = view(&core, &ReportNav::new(d(10, 1)));
    assert!(view.empty);
    assert_eq!(view.empty_title, "Nada registrado nesta semana");
    assert_eq!(view.total, "0min");
    assert!(!view.at_current);
    let Content::Weekly(week) = view.content else {
        panic!("not the week")
    };
    assert!(
        week.days
            .iter()
            .all(|c| c.total.is_empty() && c.height == 0.0)
    );
}

#[test]
fn daily_places_sessions_and_markers_on_the_hours() {
    let core = demo_core();
    let view = view(&core, &nav(Tab::Daily, d(11, 6)));
    assert!(!view.empty);
    assert_eq!(view.total, "2h 50min");
    let Content::Daily(day) = view.content else {
        panic!("not the day")
    };
    let ticks: Vec<_> = day.ticks.iter().map(|t| t.label.to_string()).collect();
    assert_eq!(ticks, ["8h", "10h", "12h", "14h", "16h"]);
    assert_eq!(day.lanes, 1);
    assert_eq!(day.blocks.len(), 3);
    let reading = &day.blocks[0];
    assert!(reading.start.abs() < 1e-6);
    assert!((reading.size - 70.0 / 540.0).abs() < 1e-6);
    let parts: Vec<_> = reading.parts.iter().collect();
    assert_eq!(parts.len(), 3);
    assert!((parts[1].start - 30.0 / 70.0).abs() < 1e-6);
    let markers: Vec<_> = day
        .markers
        .iter()
        .map(|m| (m.name.to_string(), m.time.to_string(), m.done))
        .collect();
    assert_eq!(
        markers,
        [
            ("Capítulo 3".to_owned(), "08:18".to_owned(), true),
            ("Capítulo 4".to_owned(), "08:52".to_owned(), true),
        ]
    );
}

#[test]
fn daily_lists_the_sessions_with_their_milestones() {
    let core = demo_core();
    let Content::Daily(day) = view(&core, &nav(Tab::Daily, d(11, 6))).content else {
        panic!("not the day")
    };
    let row = &day.sessions[0];
    assert_eq!(row.range, "08:00 – 09:10");
    assert_eq!(row.task, "ler 20 páginas");
    assert_eq!(row.project, "pessoal");
    assert_eq!(row.kind, "Cronômetro");
    assert_eq!(row.duration, "1h 00min");
    assert_eq!(row.markers, "✓ Capítulo 3 às 08:18 · ✓ Capítulo 4 às 08:52");
    assert_eq!(day.sessions[1].kind, "Temporizador");
    assert!(day.sessions[1].markers.is_empty());
}

#[test]
fn today_shows_the_markers_checked_outside_a_timer() {
    let core = demo_core();
    let view = view(&core, &nav(Tab::Daily, today()));
    let Content::Daily(day) = view.content else {
        panic!("not the day")
    };
    assert_eq!(day.blocks.len(), 2);
    // 10:15 to the 20:00 markers.
    let ticks: Vec<_> = day.ticks.iter().map(|t| t.label.to_string()).collect();
    assert_eq!(ticks, ["10h", "12h", "14h", "16h", "18h", "20h"]);
    assert!(day.markers.iter().all(|m| (m.at - 1.0).abs() < 1e-6));
}

#[test]
fn an_empty_day_has_the_default_hours() {
    let core = demo_core();
    let view = view(&core, &nav(Tab::Daily, d(10, 1)));
    assert!(view.empty);
    assert_eq!(view.empty_title, "Nada registrado neste dia");
    let Content::Daily(day) = view.content else {
        panic!("not the day")
    };
    assert_eq!(day.ticks.first().unwrap().label, "8h");
    assert_eq!(day.ticks.last().unwrap().label, "18h");
}

#[test]
fn overlapping_sessions_take_separate_lanes() {
    let mut core = demo_core();
    let start = Period::day(d(10, 1), &tz()).unwrap().start();
    let at = |m: u64| start.plus(Duration::from_mins(m));
    for (run, (a, b)) in [(900, (600, 660)), (901, (630, 700)), (902, (670, 720))] {
        let session = clocky_core::engine::session::CompletedSession::new(
            clocky_core::engine::session::TimerKind::Stopwatch,
            vec![clocky_core::engine::run::Segment {
                start: at(a),
                end: at(b),
            }],
        )
        .unwrap();
        core.record_session(None, clocky_core::domain::TimerId(run), &session)
            .unwrap();
    }
    let Content::Daily(day) = view(&core, &nav(Tab::Daily, d(10, 1))).content else {
        panic!("not the day")
    };
    let lanes: Vec<_> = day.blocks.iter().map(|b| b.lane).collect();
    assert_eq!(lanes, [0, 1, 0]);
    assert_eq!(day.lanes, 2);
    assert!(day.sessions.iter().all(|s| s.none && s.task == NO_TASK));
}

#[test]
fn session_detail_has_pauses_and_milestones_in_focus_minutes() {
    let core = demo_core();
    let Content::Daily(day) = view(&core, &nav(Tab::Daily, d(11, 6))).content else {
        panic!("not the day")
    };
    let id = SessionId(i64::from(day.blocks[0].id));
    let open = ReportNav {
        session: Some((id, Back::Reports)),
        ..nav(Tab::Daily, d(11, 6))
    };
    let view = view(&core, &open);
    assert_eq!(view.heading, "Sessão");
    assert_eq!(view.subheading, "sex, 6 de novembro · 08:00 – 09:10");
    let Content::Session(s) = view.content else {
        panic!("not the session")
    };
    assert_eq!(
        (s.task.as_str(), s.project.as_str(), s.kind.as_str()),
        ("ler 20 páginas", "pessoal", "Cronômetro")
    );
    assert_eq!(
        (s.effective.as_str(), s.pauses.as_str(), s.wall.as_str()),
        ("1h 00min", "2 pausas · 10min", "1h 10min")
    );
    assert_eq!(s.parts.len(), 3);
    assert_eq!(s.marks.len(), 2);
    let events: Vec<_> = s
        .events
        .iter()
        .map(|e| {
            format!("{} {} {}", e.time, e.text, e.detail)
                .trim()
                .to_owned()
        })
        .collect();
    assert_eq!(
        events,
        [
            "08:00 Início",
            "08:18 Capítulo 3 aos 18min",
            "08:25 – 08:30 Pausa 5min",
            "08:50 – 08:55 Pausa 5min",
            "08:52 Capítulo 4 aos 45min",
            "09:10 Fim",
        ]
    );
    assert_eq!(s.events[1].kind, ui::SessionEventKind::Done);
}

#[test]
fn a_missing_session_is_an_error() {
    let core = demo_core();
    let open = ReportNav {
        session: Some((SessionId(9_999), Back::Reports)),
        ..ReportNav::new(today())
    };
    assert!(build(&core, &open, today(), &tz()).is_err());
}
