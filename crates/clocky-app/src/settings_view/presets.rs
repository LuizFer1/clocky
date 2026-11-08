//! "Pomodoros salvos" in Configurações: the list of saved pomodoros, the inline editor (name
//! and the four values) and deletion. Deleting never touches recorded sessions (they keep no
//! link to the preset); a pomodoro running from it goes on (its link is cleared by the store).

use std::time::Duration;

use clocky_core::domain::{PomodoroPresetId, TaskId};
use clocky_core::store::{NewPomodoroPreset, PomodoroPreset};

use super::{Applied, WIZARD_STEPS, stepped};
use crate::core::{AppCore, CoreResult};
use crate::format;
use crate::timers::STEP_INFO;

/// Shown when the edited name is empty.
pub const EMPTY_NAME: &str = "Dê um nome ao pomodoro.";

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetRow {
    pub id: PomodoroPresetId,
    pub name: String,
    /// "25 · 5 · 15 min · 4×"
    pub summary: String,
}

/// A saved pomodoro being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetDraft {
    pub id: PomodoroPresetId,
    pub name: String,
    /// Foco, Pausa curta, Pausa longa (minutes) and Fluxos, as in the wizard.
    pub values: [u32; 4],
    // Kept as they are: the editor does not show them.
    auto_start: bool,
    task: Option<TaskId>,
}

impl PresetDraft {
    pub fn new(preset: &PomodoroPreset) -> Self {
        let minutes = |d: Duration| u32::try_from(d.as_secs().div_ceil(60)).unwrap_or(u32::MAX);
        let raw = [
            minutes(preset.focus),
            minutes(preset.short_break),
            minutes(preset.long_break),
            preset.focus_per_round,
        ];
        Self {
            id: preset.id,
            name: preset.name.clone(),
            values: std::array::from_fn(|i| clamp(i, raw[i])),
            auto_start: preset.auto_start,
            task: preset.task,
        }
    }

    /// Moves value `index` one wizard step (`direction` < 0: down), within the wizard limits.
    pub fn step(&mut self, index: usize, direction: i32) {
        let (Some(value), Some(step), Some(info)) = (
            self.values.get_mut(index),
            WIZARD_STEPS.get(index),
            STEP_INFO.get(index),
        ) else {
            return;
        };
        *value = stepped(*value, direction, *step, info.min, info.max);
    }

    pub fn at_min(&self) -> [bool; 4] {
        std::array::from_fn(|i| STEP_INFO.get(i).is_some_and(|s| self.values[i] <= s.min))
    }

    pub fn at_max(&self) -> [bool; 4] {
        std::array::from_fn(|i| STEP_INFO.get(i).is_some_and(|s| self.values[i] >= s.max))
    }

    pub fn summary(&self) -> String {
        let [focus, short, long, rounds] = self.values;
        format::pomodoro_summary(mins(focus), mins(short), mins(long), rounds)
    }

    /// The trimmed name, or [`EMPTY_NAME`].
    pub fn clean_name(&self) -> Result<&str, &'static str> {
        let name = self.name.trim();
        if name.is_empty() {
            Err(EMPTY_NAME)
        } else {
            Ok(name)
        }
    }
}

fn mins(m: u32) -> Duration {
    Duration::from_mins(u64::from(m))
}

fn clamp(index: usize, value: u32) -> u32 {
    STEP_INFO
        .get(index)
        .map_or(value, |info| value.clamp(info.min, info.max))
}

/// The saved pomodoros, in creation order.
pub fn rows(core: &AppCore) -> CoreResult<Vec<PresetRow>> {
    Ok(core
        .pomodoro_presets()?
        .iter()
        .map(|p| PresetRow {
            id: p.id,
            name: p.name.clone(),
            summary: format::pomodoro_summary(
                p.focus,
                p.short_break,
                p.long_break,
                p.focus_per_round,
            ),
        })
        .collect())
}

/// Opens the editor on the saved pomodoro `id` (`None` when it is gone).
pub fn draft(core: &AppCore, id: PomodoroPresetId) -> CoreResult<Option<PresetDraft>> {
    Ok(core
        .pomodoro_presets()?
        .iter()
        .find(|p| p.id == id)
        .map(PresetDraft::new))
}

/// Saves the edited pomodoro. An empty name is refused before the store is touched.
pub fn save(core: &mut AppCore, draft: &PresetDraft) -> anyhow::Result<Applied> {
    let name = draft.clean_name().map_err(anyhow::Error::msg)?;
    let [focus, short, long, rounds] = draft.values;
    core.update_pomodoro_preset(
        draft.id,
        &NewPomodoroPreset {
            name,
            focus: mins(focus),
            short_break: mins(short),
            long_break: mins(long),
            focus_per_round: rounds,
            auto_start: draft.auto_start,
            task: draft.task,
        },
    )?;
    Ok(Applied::Presets)
}

/// Deletes the saved pomodoro `id`.
pub fn delete(core: &mut AppCore, id: PomodoroPresetId) -> CoreResult<Applied> {
    core.delete_pomodoro_preset(id)?;
    Ok(Applied::Presets)
}

#[cfg(test)]
mod tests {
    use clocky_core::clock::FakeClock;
    use clocky_core::time::Timestamp;

    use super::*;

    fn core_with_one() -> (AppCore, PomodoroPresetId) {
        let mut core = AppCore::in_memory(Box::new(FakeClock::new(Timestamp::from_millis(0))));
        core.create_pomodoro_preset(&NewPomodoroPreset {
            name: "Clássico",
            focus: mins(25),
            short_break: mins(5),
            long_break: mins(15),
            focus_per_round: 4,
            auto_start: true,
            task: None,
        })
        .unwrap();
        let id = core.pomodoro_presets().unwrap()[0].id;
        (core, id)
    }

    #[test]
    fn rows_list_the_saved_pomodoros_with_a_summary() {
        let (core, id) = core_with_one();
        assert_eq!(
            rows(&core).unwrap(),
            vec![PresetRow {
                id,
                name: "Clássico".into(),
                summary: "25 · 5 · 15 min · 4×".into(),
            }]
        );
    }

    #[test]
    fn editing_renames_and_changes_the_values() {
        let (mut core, id) = core_with_one();
        let mut d = draft(&core, id).unwrap().unwrap();
        assert_eq!(d.values, [25, 5, 15, 4]);
        d.name = "  Deep work ".into();
        d.step(0, 1);
        d.step(1, -1);
        d.step(3, 1);
        assert_eq!(d.values, [30, 4, 15, 5]);
        assert_eq!(save(&mut core, &d).unwrap(), Applied::Presets);
        let saved = &core.pomodoro_presets().unwrap()[0];
        assert_eq!(saved.name, "Deep work");
        assert_eq!(saved.focus, mins(30));
        assert_eq!(saved.short_break, mins(4));
        assert_eq!(saved.focus_per_round, 5);
        // What the editor does not show is kept.
        assert!(saved.auto_start);
    }

    #[test]
    fn an_empty_name_is_refused() {
        let (mut core, id) = core_with_one();
        let mut d = draft(&core, id).unwrap().unwrap();
        d.name = "   ".into();
        assert_eq!(d.clean_name(), Err(EMPTY_NAME));
        assert!(save(&mut core, &d).is_err());
        assert_eq!(core.pomodoro_presets().unwrap()[0].name, "Clássico");
    }

    #[test]
    fn values_stay_within_the_wizard_limits() {
        let (core, id) = core_with_one();
        let mut d = draft(&core, id).unwrap().unwrap();
        for _ in 0..100 {
            d.step(3, 1);
            d.step(1, -1);
        }
        assert_eq!(d.values[3], STEP_INFO[3].max);
        assert_eq!(d.values[1], STEP_INFO[1].min);
        assert!(d.at_max()[3]);
        assert!(d.at_min()[1]);
        assert!(!d.at_min()[0]);
    }

    #[test]
    fn deleting_removes_it_and_a_gone_preset_has_no_draft() {
        let (mut core, id) = core_with_one();
        assert_eq!(delete(&mut core, id).unwrap(), Applied::Presets);
        assert_eq!(rows(&core).unwrap(), Vec::new());
        assert_eq!(draft(&core, id).unwrap(), None);
        assert!(delete(&mut core, id).is_err());
    }
}
