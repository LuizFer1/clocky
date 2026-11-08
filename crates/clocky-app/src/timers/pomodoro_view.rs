//! The pomodoro timer view (screens 41–43): phase chip, ring, time, cycle dots and buttons.

use std::f64::consts::TAU;

use clocky_core::engine::pomodoro::{Phase, Pomodoro, PomodoroConfig};
use clocky_core::time::Timestamp;

use super::cards::phase_label;
use super::convert::fraction;
use crate::format;

/// Ring size and stroke (handoff: 220 with stroke 8).
pub const RING_SIZE: f64 = 220.0;
pub const RING_STROKE: f64 = 8.0;

/// State of one cycle dot: filled = done, outline = current, dim outline = to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Done,
    Current,
    Todo,
}

impl Dot {
    pub fn as_index(self) -> i32 {
        match self {
            Self::Done => 2,
            Self::Current => 1,
            Self::Todo => 0,
        }
    }
}

/// What the main button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primary {
    /// Start the pomodoro (idle preset) or the waiting phase.
    Start,
    Pause,
    Resume,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PomodoroView {
    pub phase: &'static str,
    pub is_break: bool,
    pub time: String,
    pub focus_label: String,
    /// A phase ended and the next one waits: "Foco concluído. Hora de uma pausa."
    pub note: String,
    pub progress: f32,
    pub dots: Vec<Dot>,
    pub primary: Primary,
    pub primary_label: &'static str,
    /// Stop and skip only exist once the pomodoro runs.
    pub started: bool,
}

/// A running (or waiting) pomodoro at `now`.
pub fn running(p: &Pomodoro, now: Timestamp) -> PomodoroView {
    let config = p.config();
    let phase = p.phase();
    let remaining = p.remaining(now);
    let (primary, primary_label) = if p.is_waiting() {
        (Primary::Start, start_label(phase))
    } else if p.is_running() {
        (Primary::Pause, "Pausar")
    } else {
        (Primary::Resume, "Retomar")
    };
    PomodoroView {
        phase: phase_label(phase),
        is_break: phase != Phase::Focus,
        time: format::remaining_label(remaining),
        focus_label: focus_label(p.focus_index(), config.focus_per_round()),
        note: if p.is_waiting() {
            waiting_note(phase).to_owned()
        } else {
            String::new()
        },
        progress: 1.0 - fraction(remaining, config.duration(phase)),
        dots: dots(phase, p.focus_index(), config.focus_per_round()),
        primary,
        primary_label,
        started: true,
    }
}

/// A saved pomodoro that has not started yet (screen 41).
pub fn idle(config: &PomodoroConfig) -> PomodoroView {
    PomodoroView {
        phase: phase_label(Phase::Focus),
        is_break: false,
        time: format::remaining_label(config.duration(Phase::Focus)),
        focus_label: focus_label(1, config.focus_per_round()),
        note: String::new(),
        progress: 0.0,
        dots: dots(Phase::Focus, 1, config.focus_per_round()),
        primary: Primary::Start,
        primary_label: "Iniciar",
        started: false,
    }
}

fn focus_label(index: u32, total: u32) -> String {
    format!("foco {index}/{total}")
}

fn start_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Focus => "Iniciar foco",
        Phase::ShortBreak | Phase::LongBreak => "Iniciar pausa",
    }
}

fn waiting_note(phase: Phase) -> &'static str {
    match phase {
        Phase::Focus => "Pausa concluída. Hora de focar.",
        Phase::ShortBreak | Phase::LongBreak => "Foco concluído. Hora de uma pausa.",
    }
}

/// One dot per focus of the round. During a break the focus before it counts as done.
pub fn dots(phase: Phase, focus_index: u32, total: u32) -> Vec<Dot> {
    let done = if phase == Phase::Focus {
        focus_index.saturating_sub(1)
    } else {
        focus_index
    };
    (0..total)
        .map(|i| {
            if i < done {
                Dot::Done
            } else if i == done && phase == Phase::Focus {
                Dot::Current
            } else {
                Dot::Todo
            }
        })
        .collect()
}

/// SVG path of the progress arc, clockwise from the top, in a `RING_SIZE` viewbox.
pub fn arc_path(progress: f32) -> String {
    let center = RING_SIZE / 2.0;
    let radius = center - RING_STROKE / 2.0;
    let top = center - radius;
    let share = f64::from(progress).clamp(0.0, 1.0);
    if share <= 0.0 {
        return format!("M {center} {top}");
    }
    if share >= 0.9999 {
        let bottom = center + radius;
        return format!(
            "M {center} {top} A {radius} {radius} 0 1 1 {center} {bottom}              A {radius} {radius} 0 1 1 {center} {top}"
        );
    }
    let (x, y) = point_on_ring(share);
    let large = u8::from(share > 0.5);
    format!("M {center} {top} A {radius} {radius} 0 {large} 1 {x:.2} {y:.2}")
}

/// Head of the arc (the dot on the ring), in viewbox units.
pub fn arc_head(progress: f32) -> (f64, f64) {
    point_on_ring(f64::from(progress).clamp(0.0, 1.0))
}

fn point_on_ring(share: f64) -> (f64, f64) {
    let center = RING_SIZE / 2.0;
    let radius = center - RING_STROKE / 2.0;
    let angle = share * TAU;
    (center + radius * angle.sin(), center - radius * angle.cos())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_millis(secs * 1000)
    }

    #[test]
    fn dots_follow_the_round() {
        use Dot::{Current, Done, Todo};
        assert_eq!(dots(Phase::Focus, 1, 4), [Current, Todo, Todo, Todo]);
        assert_eq!(dots(Phase::ShortBreak, 1, 4), [Done, Todo, Todo, Todo]);
        assert_eq!(dots(Phase::Focus, 3, 4), [Done, Done, Current, Todo]);
        assert_eq!(dots(Phase::LongBreak, 4, 4), [Done, Done, Done, Done]);
    }

    #[test]
    fn running_focus_offers_pause() {
        let p = Pomodoro::start(PomodoroConfig::default(), at(0));
        let v = running(&p, at(3));
        assert_eq!((v.phase, v.time.as_str()), ("Foco", "24:57"));
        assert_eq!(v.focus_label, "foco 1/4");
        assert_eq!((v.primary, v.primary_label), (Primary::Pause, "Pausar"));
        assert!(v.started && !v.is_break && v.note.is_empty());
        assert!((v.progress - 3.0 / 1500.0).abs() < 1e-6);
    }

    #[test]
    fn waiting_break_offers_to_start_it() {
        let mut p = Pomodoro::start(PomodoroConfig::default(), at(0));
        assert_ne!(p.tick(at(1500)), Vec::new());
        let v = running(&p, at(1510));
        assert_eq!((v.phase, v.time.as_str()), ("Pausa curta", "05:00"));
        assert_eq!(
            (v.primary, v.primary_label),
            (Primary::Start, "Iniciar pausa")
        );
        assert_eq!(v.note, "Foco concluído. Hora de uma pausa.");
        assert!(v.is_break);
    }

    #[test]
    fn paused_offers_resume() {
        let mut p = Pomodoro::start(PomodoroConfig::default(), at(0));
        p.pause(at(60)).unwrap();
        assert_eq!(running(&p, at(90)).primary, Primary::Resume);
    }

    #[test]
    fn idle_preset_shows_its_full_focus() {
        let config = PomodoroConfig::new(
            Duration::from_mins(50),
            Duration::from_mins(10),
            Duration::from_mins(30),
            3,
            false,
        )
        .unwrap();
        let v = idle(&config);
        assert_eq!(
            (v.time.as_str(), v.focus_label.as_str()),
            ("50:00", "foco 1/3")
        );
        assert!(!v.started);
        assert_eq!(v.dots.len(), 3);
    }

    #[test]
    fn arc_goes_clockwise_from_the_top() {
        assert_eq!(arc_path(0.0), "M 110 4");
        assert_eq!(arc_path(0.25), "M 110 4 A 106 106 0 0 1 216.00 110.00");
        assert!(arc_path(0.75).contains(" 0 1 1 4.00 110.00"));
        assert!(arc_path(1.0).ends_with("A 106 106 0 1 1 110 4"));
        let (x, y) = arc_head(0.5);
        assert!((x - 110.0).abs() < 1e-9 && (y - 216.0).abs() < 1e-9);
    }
}
