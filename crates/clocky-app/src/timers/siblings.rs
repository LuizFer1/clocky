//! Several timers of one kind in its panel: "2/3" in the header with ‹ › to move between them,
//! and "+ Novo" to start another one while they run.

use clocky_core::domain::TimerId;
use clocky_core::engine::timers::Timers;
use slint::SharedString;

use super::cards::Kind;
use super::state::{PomodoroPick, State, View, pick, pick_countdown, pick_stopwatch};

/// The header controls for the timers of the open panel's kind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Siblings {
    /// "2/3" for the timer shown; "3 ativos" on a screen that starts a new one; empty when no
    /// timer of the kind is active.
    pub label: SharedString,
    /// ‹ › move to the previous or next timer of the kind.
    pub can_cycle: bool,
    /// "+ Novo": the panel shows a running timer and can start another one.
    pub can_new: bool,
}

/// The kind of timers a view is about; `None` for Ativos, the wizard and the alarm.
pub fn kind_of_view(view: View) -> Option<Kind> {
    match view {
        View::Stopwatch => Some(Kind::Stopwatch),
        View::CountdownConfig | View::CountdownRun => Some(Kind::Countdown),
        View::PomodoroList | View::PomodoroTimer => Some(Kind::Pomodoro),
        View::Hidden | View::Wizard | View::Overview | View::Alarm { .. } => None,
    }
}

/// The timer the view shows (a running, paused or ringing one); `None` on the screens that
/// start a new one (fresh stopwatch, countdown editor, pomodoro list or a saved pomodoro).
pub fn shown(state: &State, timers: &Timers) -> Option<TimerId> {
    match state.view {
        View::Stopwatch => pick_stopwatch(timers, state.stopwatch),
        View::CountdownRun => pick_countdown(timers, &state.ringing, state.countdown),
        View::PomodoroTimer => match state.pomodoro {
            PomodoroPick::Active(selected) => pick(timers, Kind::Pomodoro, selected),
            PomodoroPick::Draft(_) => None,
        },
        _ => None,
    }
}

/// Active timers of `kind`, oldest first (the order of "1/3", "2/3"...).
fn of_kind(timers: &Timers, kind: Kind) -> Vec<TimerId> {
    let mut ids: Vec<TimerId> = timers
        .iter()
        .filter(|active| Kind::of(&active.timer) == kind)
        .map(clocky_core::engine::timers::ActiveTimer::id)
        .collect();
    ids.sort();
    ids
}

pub fn siblings(state: &State, timers: &Timers) -> Siblings {
    let Some(kind) = kind_of_view(state.view) else {
        return Siblings::default();
    };
    let ids = of_kind(timers, kind);
    let current = shown(state, timers);
    let position = current.and_then(|id| ids.iter().position(|&other| other == id));
    let label = match (position, ids.len()) {
        (_, 0) => String::new(),
        (Some(i), n) => format!("{}/{n}", i + 1),
        (None, 1) => "1 ativo".into(),
        (None, n) => format!("{n} ativos"),
    };
    Siblings {
        label: label.into(),
        // From a new-timer screen, one active timer is enough to go back to it.
        can_cycle: ids.len() > 1 || (position.is_none() && !ids.is_empty()),
        can_new: current.is_some(),
    }
}

/// The timer ‹ (`forward = false`) or › goes to: the neighbour of `current` among the active
/// timers of `kind`, wrapping around. From a timer that is not among them (none, or a ringing
/// countdown), › goes to the oldest and ‹ to the newest.
pub fn cycle_target(
    timers: &Timers,
    kind: Kind,
    current: Option<TimerId>,
    forward: bool,
) -> Option<TimerId> {
    let ids = of_kind(timers, kind);
    let last = ids.len().checked_sub(1)?;
    let next = match current.and_then(|id| ids.iter().position(|&other| other == id)) {
        Some(i) if forward => (i + 1) % ids.len(),
        Some(0) => last,
        Some(i) => i - 1,
        None if forward => 0,
        None => last,
    };
    ids.get(next).copied()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use clocky_core::engine::countdown::Countdown;
    use clocky_core::engine::stopwatch::Stopwatch;
    use clocky_core::engine::timers::Timer;
    use clocky_core::time::Timestamp;

    use super::super::state::StopwatchPick;
    use super::*;

    fn timers() -> (Timers, [TimerId; 3]) {
        let t0 = Timestamp::from_millis(0);
        let mut timers = Timers::new();
        let a = timers.add(None, Timer::Stopwatch(Stopwatch::start(t0)));
        let b = timers.add(None, Timer::Stopwatch(Stopwatch::start(t0)));
        let c = timers.add(
            None,
            Timer::Countdown(Countdown::start(t0, Duration::from_mins(5))),
        );
        (timers, [a, b, c])
    }

    fn stopwatch_state(pick: StopwatchPick) -> State {
        let mut state = State::new();
        state.view = View::Stopwatch;
        state.stopwatch = pick;
        state
    }

    #[test]
    fn a_shown_timer_has_its_position_and_can_get_a_sibling() {
        let (timers, [a, b, _]) = timers();
        let state = stopwatch_state(StopwatchPick::Active(Some(a)));
        let found = siblings(&state, &timers);
        assert_eq!(found.label, "1/2");
        assert!(found.can_cycle && found.can_new);
        // No selection: the most recent one.
        let state = stopwatch_state(StopwatchPick::Active(None));
        assert_eq!(shown(&state, &timers), Some(b));
        assert_eq!(siblings(&state, &timers).label, "2/2");
    }

    #[test]
    fn a_fresh_stopwatch_counts_the_running_ones_and_offers_no_new() {
        let (timers, _) = timers();
        let found = siblings(&stopwatch_state(StopwatchPick::Fresh), &timers);
        assert_eq!(found.label, "2 ativos");
        assert!(found.can_cycle && !found.can_new);
    }

    #[test]
    fn a_single_timer_cycles_only_from_a_new_timer_screen() {
        let (timers, [_, _, c]) = timers();
        let mut state = State::new();
        state.view = View::CountdownRun;
        state.countdown = Some(c);
        let found = siblings(&state, &timers);
        assert_eq!(found.label, "1/1");
        assert!(!found.can_cycle && found.can_new);
        state.view = View::CountdownConfig;
        let found = siblings(&state, &timers);
        assert_eq!(found.label, "1 ativo");
        assert!(found.can_cycle && !found.can_new);
    }

    #[test]
    fn panels_without_a_kind_or_timers_show_nothing() {
        let (timers, _) = timers();
        let mut state = State::new();
        state.view = View::Overview;
        assert_eq!(siblings(&state, &timers), Siblings::default());
        state.view = View::PomodoroList;
        assert_eq!(siblings(&state, &timers), Siblings::default());
    }

    #[test]
    fn cycling_wraps_and_enters_from_either_end() {
        let (timers, [a, b, c]) = timers();
        let sw = Kind::Stopwatch;
        assert_eq!(cycle_target(&timers, sw, Some(a), true), Some(b));
        assert_eq!(cycle_target(&timers, sw, Some(b), true), Some(a));
        assert_eq!(cycle_target(&timers, sw, Some(a), false), Some(b));
        assert_eq!(cycle_target(&timers, sw, Some(b), false), Some(a));
        assert_eq!(cycle_target(&timers, sw, None, true), Some(a));
        assert_eq!(cycle_target(&timers, sw, None, false), Some(b));
        assert_eq!(
            cycle_target(&timers, Kind::Countdown, Some(c), true),
            Some(c)
        );
        assert_eq!(cycle_target(&timers, Kind::Pomodoro, None, true), None);
    }
}
