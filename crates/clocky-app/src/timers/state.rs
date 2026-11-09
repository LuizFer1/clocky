//! What the timer panels remember between ticks: the open view, which timer each panel shows,
//! the unsaved configuration (h/m/s, wizard, draft tasks) and the ringing countdowns.

use std::collections::BTreeSet;

use clocky_core::domain::{PomodoroPresetId, TaskId, TimerId};
use clocky_core::engine::timers::Timers;

use super::cards::{Kind, Ringing};
use super::hms::Hms;
use super::wizard::{Defaults, Wizard};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Not a timer panel (Alarme): the panel window shows its placeholder body.
    Hidden,
    Stopwatch,
    CountdownConfig,
    CountdownRun,
    PomodoroList,
    PomodoroTimer,
    Wizard,
    Overview,
    /// M7a: the Alarme panel; back returns to Ativos when a card opened it.
    Alarm {
        from_overview: bool,
    },
}

impl View {
    /// Where the header's back button goes inside the panel; `None` = back to the menu.
    pub fn parent(self) -> Option<Self> {
        match self {
            Self::CountdownRun => Some(Self::CountdownConfig),
            Self::PomodoroTimer | Self::Wizard => Some(Self::PomodoroList),
            Self::Alarm {
                from_overview: true,
            } => Some(Self::Overview),
            _ => None,
        }
    }
}

/// What the Cronômetro shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopwatchPick {
    /// The selected active stopwatch, else the most recent one.
    Active(Option<TimerId>),
    /// "+ Novo": a ready stopwatch, so "Iniciar" starts another one next to the running ones.
    Fresh,
}

/// What the pomodoro timer view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomodoroPick {
    /// The selected active pomodoro, else the most recent one.
    Active(Option<TimerId>),
    /// A saved pomodoro that has not started (screen 41).
    Draft(PomodoroPresetId),
}

pub struct State {
    pub view: View,
    pub stopwatch: StopwatchPick,
    pub countdown: Option<TimerId>,
    pub pomodoro: PomodoroPick,
    /// Task of the next stopwatch, countdown and draft pomodoro.
    pub stopwatch_task: Option<TaskId>,
    pub countdown_task: Option<TaskId>,
    pub pomodoro_task: Option<TaskId>,
    pub hms: Hms,
    pub wizard: Wizard,
    /// The values a new wizard starts from.
    pub wizard_defaults: Defaults,
    pub ringing: Vec<Ringing>,
    pub notice: String,
    /// The notice was on screen: the next panel opened from the menu clears it.
    pub notice_seen: bool,
    /// The forgotten dialog on screen, and the stopwatches already asked about.
    pub forgotten: Option<TimerId>,
    pub forgotten_asked: BTreeSet<TimerId>,
    /// A tick failed and the failure was logged; cleared by the next good tick.
    pub tick_failing: bool,
    /// The forgotten-stopwatch check is failing (logged once per streak).
    pub forgotten_failing: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            view: View::Hidden,
            stopwatch: StopwatchPick::Active(None),
            countdown: None,
            pomodoro: PomodoroPick::Active(None),
            stopwatch_task: None,
            countdown_task: None,
            pomodoro_task: None,
            hms: Hms::from_secs(12 * 60),
            wizard: Wizard::new(Defaults::default()),
            wizard_defaults: Defaults::default(),
            ringing: Vec::new(),
            notice: String::new(),
            notice_seen: false,
            forgotten: None,
            forgotten_asked: BTreeSet::new(),
            tick_failing: false,
            forgotten_failing: false,
        }
    }

    pub fn ringing(&self, id: TimerId) -> Option<&Ringing> {
        self.ringing.iter().find(|r| r.timer == id)
    }
}

/// The timer of `kind` a view shows: the selected one while it is active, none once it ended,
/// and the most recent one when nothing is selected. A view whose timer ended goes back to its
/// root instead of jumping to another timer of that kind, so a second click on "Parar" (a
/// double click, or two timers of the same kind) never stops another timer.
pub fn pick(timers: &Timers, kind: Kind, selected: Option<TimerId>) -> Option<TimerId> {
    let of_kind = |id: TimerId| {
        timers
            .get(id)
            .is_some_and(|active| Kind::of(&active.timer) == kind)
    };
    match selected {
        Some(id) => of_kind(id).then_some(id),
        None => timers
            .iter()
            .filter(|active| Kind::of(&active.timer) == kind)
            .map(clocky_core::engine::timers::ActiveTimer::id)
            .max(),
    }
}

/// The stopwatch the Cronômetro shows: none for a fresh one, else as [`pick`].
pub fn pick_stopwatch(timers: &Timers, selected: StopwatchPick) -> Option<TimerId> {
    match selected {
        StopwatchPick::Active(selected) => pick(timers, Kind::Stopwatch, selected),
        StopwatchPick::Fresh => None,
    }
}

/// The countdown the Temporizador shows: the selected one while it rings or runs (none once it
/// ended, as [`pick`]); with no selection, a ringing one, then the most recent active one.
pub fn pick_countdown(
    timers: &Timers,
    ringing: &[Ringing],
    selected: Option<TimerId>,
) -> Option<TimerId> {
    match selected {
        Some(id) if ringing.iter().any(|r| r.timer == id) => Some(id),
        Some(id) => pick(timers, Kind::Countdown, Some(id)),
        None => ringing
            .last()
            .map(|r| r.timer)
            .or_else(|| pick(timers, Kind::Countdown, None)),
    }
}

/// Selects `id` in its panel (`kind`), so the view stays on it, and on its end state once it
/// stops, instead of moving to another timer of that kind. A saved pomodoro opened but not
/// started yet (a draft) stays selected: stopping another pomodoro (from Ativos) keeps it.
pub fn select(state: &mut State, kind: Kind, id: TimerId) {
    match kind {
        Kind::Stopwatch => state.stopwatch = StopwatchPick::Active(Some(id)),
        Kind::Countdown => state.countdown = Some(id),
        Kind::Pomodoro => {
            if let PomodoroPick::Active(_) = state.pomodoro {
                state.pomodoro = PomodoroPick::Active(Some(id));
            }
        }
    }
}

/// Drops every draft task (and ringing card task) that no longer exists, deleted in Gerenciar or
/// elsewhere: a timer started with it would fail on the database's foreign key.
pub fn forget_missing_tasks(state: &mut State, exists: impl Fn(TaskId) -> bool) {
    let drafts = [
        &mut state.stopwatch_task,
        &mut state.countdown_task,
        &mut state.pomodoro_task,
        &mut state.wizard.task,
    ];
    for task in drafts {
        if task.is_some_and(|t| !exists(t)) {
            *task = None;
        }
    }
    for ringing in &mut state.ringing {
        if ringing.task.is_some_and(|t| !exists(t)) {
            ringing.task = None;
        }
    }
}

/// The draft and ringing tasks of `state`, to check against the database.
pub fn draft_tasks(state: &State) -> Vec<TaskId> {
    [
        state.stopwatch_task,
        state.countdown_task,
        state.pomodoro_task,
        state.wizard.task,
    ]
    .into_iter()
    .chain(state.ringing.iter().map(|r| r.task))
    .flatten()
    .collect()
}

/// Drops the selections whose timer ended: a panel opened from the menu shows the most recent
/// timer of its kind again.
pub fn forget_ended(state: &mut State, timers: &Timers) {
    let gone = |id: Option<TimerId>| id.is_some_and(|id| timers.get(id).is_none());
    if let StopwatchPick::Active(selected) = state.stopwatch
        && gone(selected)
    {
        state.stopwatch = StopwatchPick::Active(None);
    }
    let ringing =
        |id: Option<TimerId>| id.is_some_and(|id| state.ringing.iter().any(|r| r.timer == id));
    if gone(state.countdown) && !ringing(state.countdown) {
        state.countdown = None;
    }
    if let PomodoroPick::Active(selected) = state.pomodoro
        && gone(selected)
    {
        state.pomodoro = PomodoroPick::Active(None);
    }
}

/// Falls back to the panel root when the timer a view shows is gone (stopped, finished).
pub fn normalize(state: &mut State, timers: &Timers) {
    match state.view {
        View::CountdownRun if pick_countdown(timers, &state.ringing, state.countdown).is_none() => {
            state.view = View::CountdownConfig;
        }
        View::PomodoroTimer => {
            if let PomodoroPick::Active(selected) = state.pomodoro
                && pick(timers, Kind::Pomodoro, selected).is_none()
            {
                state.view = View::PomodoroList;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use clocky_core::engine::countdown::Countdown;
    use clocky_core::engine::stopwatch::Stopwatch;
    use clocky_core::engine::timers::Timer;
    use clocky_core::time::Timestamp;

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

    #[test]
    fn selection_wins_while_active_else_the_most_recent() {
        let (timers, [a, b, c]) = timers();
        assert_eq!(pick(&timers, Kind::Stopwatch, Some(a)), Some(a));
        assert_eq!(pick(&timers, Kind::Stopwatch, None), Some(b));
        // A selection that is not an active timer of that kind shows none.
        assert_eq!(pick(&timers, Kind::Stopwatch, Some(c)), None);
        assert_eq!(pick(&timers, Kind::Pomodoro, None), None);
    }

    #[test]
    fn stopping_a_pomodoro_keeps_a_pending_draft() {
        let mut state = State::new();
        state.pomodoro = PomodoroPick::Draft(PomodoroPresetId(3));
        select(&mut state, Kind::Pomodoro, TimerId(9));
        assert_eq!(state.pomodoro, PomodoroPick::Draft(PomodoroPresetId(3)));
        state.pomodoro = PomodoroPick::Active(None);
        select(&mut state, Kind::Pomodoro, TimerId(9));
        assert_eq!(state.pomodoro, PomodoroPick::Active(Some(TimerId(9))));
    }

    #[test]
    fn drafts_forget_a_deleted_task_and_keep_the_others() {
        let (deleted, kept) = (TaskId(7), TaskId(8));
        let mut state = State::new();
        state.stopwatch_task = Some(deleted);
        state.countdown_task = Some(kept);
        state.pomodoro_task = Some(deleted);
        state.wizard.task = Some(deleted);
        assert_eq!(draft_tasks(&state).len(), 4);
        forget_missing_tasks(&mut state, |t| t != deleted);
        assert_eq!(state.stopwatch_task, None);
        assert_eq!(state.countdown_task, Some(kept));
        assert_eq!(state.pomodoro_task, None);
        assert_eq!(state.wizard.task, None);
        assert_eq!(draft_tasks(&state), [kept]);
    }

    #[test]
    fn a_stopped_selection_never_moves_to_another_timer_of_its_kind() {
        let (mut timers, [a, b, c]) = timers();
        let mut state = State::new();
        select(&mut state, Kind::Stopwatch, a);
        select(&mut state, Kind::Countdown, c);
        // "Parar" on `a` and "Cancelar" on `c`: a second click finds nothing to stop.
        timers.discard(a).unwrap();
        timers.discard(c).unwrap();
        assert_eq!(pick_stopwatch(&timers, state.stopwatch), None);
        assert_eq!(pick_countdown(&timers, &[], state.countdown), None);
        // Opening the panel again from the menu shows the remaining stopwatch.
        forget_ended(&mut state, &timers);
        assert_eq!(pick_stopwatch(&timers, state.stopwatch), Some(b));
        assert_eq!(state.countdown, None);
    }

    #[test]
    fn a_fresh_stopwatch_shows_none_and_survives_ended_timers() {
        let (mut timers, [a, _, _]) = timers();
        let mut state = State::new();
        state.stopwatch = StopwatchPick::Fresh;
        // "Iniciar" on it starts a new stopwatch instead of pausing a running one.
        assert_eq!(pick_stopwatch(&timers, state.stopwatch), None);
        timers.discard(a).unwrap();
        forget_ended(&mut state, &timers);
        assert_eq!(state.stopwatch, StopwatchPick::Fresh);
        // Selecting a stopwatch (a card, ‹ ›) leaves the fresh one.
        select(&mut state, Kind::Stopwatch, a);
        assert_eq!(state.stopwatch, StopwatchPick::Active(Some(a)));
    }

    #[test]
    fn ringing_countdowns_come_before_running_ones() {
        let (timers, [_, _, c]) = timers();
        let ringing = Ringing {
            timer: TimerId(99),
            name: "chá".into(),
            task: None,
            at: Timestamp::from_millis(0),
            while_closed: false,
        };
        assert_eq!(pick_countdown(&timers, &[], None), Some(c));
        assert_eq!(
            pick_countdown(&timers, std::slice::from_ref(&ringing), None),
            Some(TimerId(99))
        );
        assert_eq!(
            pick_countdown(&timers, std::slice::from_ref(&ringing), Some(c)),
            Some(c)
        );
    }

    #[test]
    fn views_fall_back_when_their_timer_is_gone() {
        let mut state = State::new();
        state.view = View::CountdownRun;
        normalize(&mut state, &Timers::new());
        assert_eq!(state.view, View::CountdownConfig);
        state.view = View::PomodoroTimer;
        normalize(&mut state, &Timers::new());
        assert_eq!(state.view, View::PomodoroList);
        let (timers, _) = timers();
        state.view = View::CountdownRun;
        normalize(&mut state, &timers);
        assert_eq!(state.view, View::CountdownRun);
    }

    #[test]
    fn back_goes_to_the_panel_root() {
        assert_eq!(View::CountdownRun.parent(), Some(View::CountdownConfig));
        assert_eq!(View::Wizard.parent(), Some(View::PomodoroList));
        assert_eq!(View::PomodoroTimer.parent(), Some(View::PomodoroList));
        assert_eq!(View::Overview.parent(), None);
    }
}
