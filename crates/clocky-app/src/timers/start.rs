//! "Iniciar pomodoro" / "Iniciar cronômetro" on a task page of the manage window.

use std::time::Duration;

use clocky_core::domain::{PomodoroPresetId, TaskId};
use clocky_core::engine::pomodoro::PomodoroConfig;
use clocky_core::store::PomodoroPreset;

use super::TimerPanels;
use super::frame_pomodoro::preset_config;
use super::state::{PomodoroPick, View};
use super::wizard::Defaults;
use crate::manage::TimerStart;
use crate::panel::PanelKind;

/// Shown when the wizard defaults in the settings do not make a valid pomodoro.
pub const INVALID_DEFAULTS: &str =
    "Os valores padrão do pomodoro são inválidos. Crie um pomodoro em “Novo pomodoro”.";

/// The pomodoro a task starts: its first saved pomodoro, else the wizard defaults.
pub fn task_pomodoro(
    saved: &[PomodoroPreset],
    defaults: Defaults,
) -> Option<(PomodoroConfig, Option<PomodoroPresetId>)> {
    if let Some((preset, config)) = saved.iter().find_map(|p| preset_config(p).map(|c| (p, c))) {
        return Some((config, Some(preset.id)));
    }
    let mins = |m: u32| Duration::from_mins(u64::from(m));
    PomodoroConfig::new(
        mins(defaults.focus),
        mins(defaults.short),
        mins(defaults.long),
        defaults.rounds,
        defaults.auto_start,
    )
    .inspect_err(|e| tracing::warn!("invalid pomodoro defaults: {e}"))
    .ok()
    .map(|config| (config, None))
}

impl TimerPanels {
    /// Starts `kind` with `task` and selects it; returns the panel to open. When the start
    /// fails, that panel opens with the notice saying so, where the user clicked.
    pub fn start_for_task(&self, kind: TimerStart, task: TaskId) -> PanelKind {
        match kind {
            TimerStart::Stopwatch => {
                let started = self.command("start stopwatch", |t, now| {
                    t.start_stopwatch(Some(task), now)
                });
                if let Some(id) = started {
                    let mut state = self.state.borrow_mut();
                    state.stopwatch = Some(id);
                    state.view = View::Stopwatch;
                }
                PanelKind::Stopwatch
            }
            TimerStart::Pomodoro => {
                let saved = {
                    let core = self.core.borrow();
                    let links = core.tracker().store().task_links(task);
                    links
                        .inspect_err(|e| tracing::warn!("cannot read the task's pomodoros: {e}"))
                        .map(|l| l.pomodoro_presets)
                        .unwrap_or_default()
                };
                let defaults = self.state.borrow().wizard_defaults;
                let Some((config, preset)) = task_pomodoro(&saved, defaults) else {
                    let mut state = self.state.borrow_mut();
                    INVALID_DEFAULTS.clone_into(&mut state.notice);
                    state.notice_seen = false;
                    return PanelKind::Pomodoro;
                };
                let started = self.command("start pomodoro", |t, now| {
                    t.start_pomodoro(Some(task), config, preset, now)
                });
                if let Some(id) = started {
                    let mut state = self.state.borrow_mut();
                    state.pomodoro = PomodoroPick::Active(Some(id));
                    state.view = View::PomodoroTimer;
                }
                PanelKind::Pomodoro
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(id: i64, focus: u64, rounds: u32) -> PomodoroPreset {
        PomodoroPreset {
            id: PomodoroPresetId(id),
            name: "p".into(),
            focus: Duration::from_mins(focus),
            short_break: Duration::from_mins(5),
            long_break: Duration::from_mins(15),
            focus_per_round: rounds,
            auto_start: false,
            task: None,
        }
    }

    #[test]
    fn a_task_with_a_saved_pomodoro_starts_it() {
        let (config, id) = task_pomodoro(&[preset(7, 50, 3)], Defaults::default()).unwrap();
        assert_eq!(id, Some(PomodoroPresetId(7)));
        assert_eq!(config, preset_config(&preset(7, 50, 3)).unwrap());
    }

    #[test]
    fn an_invalid_saved_pomodoro_is_skipped() {
        let (_, id) =
            task_pomodoro(&[preset(1, 50, 0), preset(2, 25, 4)], Defaults::default()).unwrap();
        assert_eq!(id, Some(PomodoroPresetId(2)));
    }

    #[test]
    fn invalid_defaults_start_nothing() {
        let defaults = Defaults {
            rounds: 0,
            ..Defaults::default()
        };
        assert!(task_pomodoro(&[], defaults).is_none());
    }

    #[test]
    fn otherwise_the_wizard_defaults() {
        let (config, id) = task_pomodoro(&[], Defaults::default()).unwrap();
        assert_eq!(id, None);
        let expected = PomodoroConfig::new(
            Duration::from_mins(25),
            Duration::from_mins(5),
            Duration::from_mins(15),
            4,
            false,
        )
        .unwrap();
        assert_eq!(config, expected);
    }
}
