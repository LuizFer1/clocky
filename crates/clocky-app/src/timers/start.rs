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
    /// Starts `kind` with `task` and selects it; returns the panel to open (`None` if the
    /// tracker refused, which the notice explains).
    pub fn start_for_task(&self, kind: TimerStart, task: TaskId) -> Option<PanelKind> {
        match kind {
            TimerStart::Stopwatch => {
                let id = self.command("start stopwatch", |t, now| {
                    t.start_stopwatch(Some(task), now)
                })?;
                let mut state = self.state.borrow_mut();
                state.stopwatch = Some(id);
                state.view = View::Stopwatch;
                Some(PanelKind::Stopwatch)
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
                let (config, preset) = task_pomodoro(&saved, defaults)?;
                let id = self.command("start pomodoro", |t, now| {
                    t.start_pomodoro(Some(task), config, preset, now)
                })?;
                let mut state = self.state.borrow_mut();
                state.pomodoro = PomodoroPick::Active(Some(id));
                state.view = View::PomodoroTimer;
                Some(PanelKind::Pomodoro)
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
