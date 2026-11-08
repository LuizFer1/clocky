//! Cards of the Ativos list (screen 50): what each timer shows and the urgency order.

use std::time::Duration;

use clocky_core::domain::{TaskId, TimerId};
use clocky_core::engine::countdown::Countdown;
use clocky_core::engine::pomodoro::{Phase, Pomodoro};
use clocky_core::engine::timers::{ActiveTimer, Timer};
use clocky_core::time::Timestamp;

use super::convert::fraction;
use super::format;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Stopwatch,
    Countdown,
    Pomodoro,
}

impl Kind {
    pub fn of(timer: &Timer) -> Self {
        match timer {
            Timer::Stopwatch(_) => Self::Stopwatch,
            Timer::Countdown(_) => Self::Countdown,
            Timer::Pomodoro(_) => Self::Pomodoro,
        }
    }

    /// The Slint name of the kind (selects the icon).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopwatch => "stopwatch",
            Self::Countdown => "countdown",
            Self::Pomodoro => "pomodoro",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Paused,
    /// A pomodoro phase waiting for the user to start it.
    Waiting,
    /// A finished countdown that was not dismissed yet ("tocando").
    Ringing,
}

/// A countdown that finished and keeps ringing until "Dispensar". The core already recorded it
/// and dropped the timer; the app keeps this until the user dismisses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ringing {
    pub timer: TimerId,
    pub name: String,
    pub task: Option<TaskId>,
    pub at: Timestamp,
    /// It came due while the app was closed: the card says "terminou às HH:MM".
    pub while_closed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: TimerId,
    pub kind: Kind,
    pub state: State,
    pub name: String,
    pub status: String,
    pub time: String,
    /// Elapsed share of a countdown or pomodoro phase (`None` = no bar).
    pub progress: Option<f32>,
    pub remaining: Duration,
    pub task: Option<TaskId>,
}

pub fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Focus => "Foco",
        Phase::ShortBreak => "Pausa curta",
        Phase::LongBreak => "Pausa longa",
    }
}

/// The display name: the preset name, else the kind ("Pomodoro · Clássico" for pomodoros).
pub fn timer_name(kind: Kind, preset: Option<&str>) -> String {
    match (kind, preset) {
        (Kind::Stopwatch, _) => "Cronômetro".to_owned(),
        (Kind::Countdown, Some(name)) => name.to_owned(),
        (Kind::Countdown, None) => "Temporizador".to_owned(),
        (Kind::Pomodoro, Some(name)) => format!("Pomodoro · {name}"),
        (Kind::Pomodoro, None) => "Pomodoro".to_owned(),
    }
}

/// The card of an active timer at `now`.
pub fn card(active: &ActiveTimer, now: Timestamp, preset: Option<&str>) -> Card {
    let kind = Kind::of(&active.timer);
    let mut card = Card {
        id: active.id(),
        kind,
        state: State::Running,
        name: timer_name(kind, preset),
        status: String::new(),
        time: String::new(),
        progress: None,
        remaining: Duration::ZERO,
        task: active.task,
    };
    match &active.timer {
        Timer::Stopwatch(sw) => {
            card.state = running_or_paused(sw.is_running());
            card.status = paused_prefix(card.state, "decorrido");
            card.time = format::elapsed_label(sw.elapsed(now));
        }
        Timer::Countdown(cd) => fill_countdown(&mut card, cd, now),
        Timer::Pomodoro(p) => fill_pomodoro(&mut card, p, now),
    }
    card
}

fn fill_countdown(card: &mut Card, cd: &Countdown, now: Timestamp) {
    let remaining = cd.remaining(now);
    card.state = running_or_paused(cd.is_running());
    card.status = paused_prefix(card.state, "restante");
    card.time = format::remaining_label(remaining);
    card.remaining = remaining;
    card.progress = Some(1.0 - fraction(remaining, cd.target()));
}

fn fill_pomodoro(card: &mut Card, p: &Pomodoro, now: Timestamp) {
    let remaining = p.remaining(now);
    let focus = format!("foco {}/{}", p.focus_index(), p.config().focus_per_round());
    let phase = phase_label(p.phase());
    card.state = if p.is_waiting() {
        State::Waiting
    } else {
        running_or_paused(p.is_running())
    };
    card.status = match card.state {
        State::Waiting => format!("{phase} · aguardando"),
        State::Paused => format!("Pausado · {focus}"),
        _ => format!("{phase} · {focus}"),
    };
    card.time = format::remaining_label(remaining);
    card.remaining = remaining;
    card.progress = Some(1.0 - fraction(remaining, p.config().duration(p.phase())));
}

fn running_or_paused(running: bool) -> State {
    if running {
        State::Running
    } else {
        State::Paused
    }
}

fn paused_prefix(state: State, text: &str) -> String {
    if state == State::Paused {
        format!("Pausado · {text}")
    } else {
        text.to_owned()
    }
}

/// The card of a ringing countdown.
pub fn ringing_card(ringing: &Ringing, local_hh_mm: &str) -> Card {
    Card {
        id: ringing.timer,
        kind: Kind::Countdown,
        state: State::Ringing,
        name: ringing.name.clone(),
        status: if ringing.while_closed {
            format!("terminou às {local_hh_mm}")
        } else {
            "Tempo esgotado".to_owned()
        },
        time: "00:00".to_owned(),
        progress: None,
        remaining: Duration::ZERO,
        task: ringing.task,
    }
}

/// Lower is more urgent: ringing, then pomodoros waiting for a click, then running and paused
/// countdowns and pomodoros by remaining time, then stopwatches.
fn rank(card: &Card) -> (u8, Duration) {
    match (card.state, card.kind) {
        (State::Ringing, _) => (0, Duration::ZERO),
        (State::Waiting, _) => (1, Duration::ZERO),
        (State::Running, Kind::Stopwatch) => (4, Duration::ZERO),
        (State::Paused, Kind::Stopwatch) => (5, Duration::ZERO),
        (State::Running, _) => (2, card.remaining),
        (State::Paused, _) => (3, card.remaining),
    }
}

pub fn sort_by_urgency(cards: &mut [Card]) {
    cards.sort_by_key(|card| (rank(card), card.id));
}

#[cfg(test)]
mod tests {
    use clocky_core::engine::pomodoro::PomodoroConfig;
    use clocky_core::engine::stopwatch::Stopwatch;

    use super::*;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_millis(secs * 1000)
    }

    fn active(id: u64, timer: Timer) -> ActiveTimer {
        ActiveTimer::new(TimerId(id), None, None, timer)
    }

    fn countdown(id: u64, start: i64, mins: u64) -> ActiveTimer {
        active(
            id,
            Timer::Countdown(Countdown::start(at(start), Duration::from_mins(mins))),
        )
    }

    #[test]
    fn stopwatch_card_counts_up() {
        let mut sw = Stopwatch::start(at(0));
        let running = card(&active(1, Timer::Stopwatch(sw.clone())), at(4_330), None);
        assert_eq!(
            (
                running.name.as_str(),
                running.status.as_str(),
                running.time.as_str()
            ),
            ("Cronômetro", "decorrido", "01:12:10")
        );
        assert_eq!(running.progress, None);
        sw.pause(at(10)).unwrap();
        let paused = card(&active(1, Timer::Stopwatch(sw)), at(99), None);
        assert_eq!(
            (paused.state, paused.status.as_str()),
            (State::Paused, "Pausado · decorrido")
        );
    }

    #[test]
    fn countdown_card_shows_remaining_and_elapsed_share() {
        let c = card(&countdown(2, 0, 10), at(150), Some("pipeline"));
        assert_eq!(c.name, "pipeline");
        assert_eq!((c.status.as_str(), c.time.as_str()), ("restante", "07:30"));
        assert!((c.progress.unwrap() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn pomodoro_card_names_phase_and_focus_index() {
        let p = Pomodoro::start(PomodoroConfig::default(), at(0));
        let c = card(&active(3, Timer::Pomodoro(p)), at(630), Some("Clássico"));
        assert_eq!(c.name, "Pomodoro · Clássico");
        assert_eq!(
            (c.status.as_str(), c.time.as_str()),
            ("Foco · foco 1/4", "14:30")
        );
    }

    #[test]
    fn waiting_pomodoro_says_so() {
        let mut p = Pomodoro::start(PomodoroConfig::default(), at(0));
        assert_ne!(p.tick(at(25 * 60)), Vec::new());
        let c = card(&active(4, Timer::Pomodoro(p)), at(25 * 60 + 5), None);
        assert_eq!(c.state, State::Waiting);
        assert_eq!(c.status, "Pausa curta · aguardando");
        assert_eq!(c.time, "05:00");
    }

    #[test]
    fn ringing_card_says_when_it_finished_while_closed() {
        let ringing = Ringing {
            timer: TimerId(9),
            name: "chá".into(),
            task: None,
            at: at(0),
            while_closed: true,
        };
        assert_eq!(ringing_card(&ringing, "14:32").status, "terminou às 14:32");
        let live = Ringing {
            while_closed: false,
            ..ringing
        };
        let c = ringing_card(&live, "14:32");
        assert_eq!(
            (c.status.as_str(), c.time.as_str()),
            ("Tempo esgotado", "00:00")
        );
    }

    #[test]
    fn urgency_puts_ringing_first_and_stopwatches_last() {
        let sw = card(
            &active(1, Timer::Stopwatch(Stopwatch::start(at(0)))),
            at(10),
            None,
        );
        let long = card(&countdown(2, 0, 30), at(10), None);
        let short = card(&countdown(3, 0, 5), at(10), None);
        let mut paused_cd = Countdown::start(at(0), Duration::from_mins(1));
        paused_cd.pause(at(5)).unwrap();
        let paused = card(&active(5, Timer::Countdown(paused_cd)), at(10), None);
        let ringing = ringing_card(
            &Ringing {
                timer: TimerId(4),
                name: "chá".into(),
                task: None,
                at: at(0),
                while_closed: false,
            },
            "",
        );
        let mut cards = vec![sw, long, paused, short, ringing];
        sort_by_urgency(&mut cards);
        let order: Vec<u64> = cards.iter().map(|c| c.id.0).collect();
        assert_eq!(order, [4, 3, 2, 5, 1]);
    }

    #[test]
    fn names_fall_back_to_the_kind() {
        assert_eq!(timer_name(Kind::Countdown, None), "Temporizador");
        assert_eq!(timer_name(Kind::Pomodoro, None), "Pomodoro");
        assert_eq!(timer_name(Kind::Stopwatch, Some("x")), "Cronômetro");
        assert_eq!(Kind::Pomodoro.as_str(), "pomodoro");
    }
}
