//! What the Configurações page shows and what its controls write. Every value lives in the
//! core `settings` table and is saved on change (there is no "Salvar" button):
//!
//! | Control | Key |
//! |---|---|
//! | Tema | `theme` (`dark` / `light`) |
//! | Cronômetro esquecido | `forgotten_limit_ms` (core) |
//! | Novo pomodoro | `pomodoro_focus_min`, `pomodoro_short_min`, `pomodoro_long_min`, `pomodoro_rounds` |
//! | Som padrão do alarme | `alarm.default_sound` (empty = the built-in sound) |
//!
//! "Iniciar com o sistema" is not stored: the OS entry is the truth ([`crate::autostart`]).

use std::time::Duration;

use crate::core::{AppCore, CoreResult};
use crate::settings::{self, keys};
use crate::timers::{STEP_INFO, WIZARD_KEYS, WizardDefaults};

/// Step and limits of the "Cronômetro esquecido" limit, in minutes.
pub const FORGOTTEN_STEP: u32 = 30;
pub const FORGOTTEN_MIN: u32 = 30;
pub const FORGOTTEN_MAX: u32 = 24 * 60;

/// Step of each wizard value (Foco, Pausa, Longa, Fluxos).
pub const WIZARD_STEPS: [u32; 4] = [5, 1, 5, 1];

/// The page's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsForm {
    pub dark: bool,
    /// Minutes before a running stopwatch counts as forgotten.
    pub forgotten_minutes: u32,
    /// Foco, Pausa curta, Pausa longa (minutes) and Fluxos.
    pub wizard: [u32; 4],
    /// Path of the default alarm sound; empty = the built-in sound.
    pub alarm_sound: String,
}

/// What a change needs from the rest of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Every window switches theme.
    Theme(bool),
    /// The timer panels read the wizard defaults again.
    Wizard,
    /// Nothing else (the forgotten limit and the alarm sound are read when used).
    Stored,
}

impl SettingsForm {
    /// Reads the table; missing or invalid values give the defaults.
    pub fn load(core: &AppCore) -> Self {
        let base = WizardDefaults::default();
        let fallback = [base.focus, base.short, base.long, base.rounds];
        let wizard = std::array::from_fn(|i| {
            core.setting(WIZARD_KEYS[i])
                .ok()
                .flatten()
                .and_then(|v| v.trim().parse().ok())
                .map_or(fallback[i], |v| clamp_wizard(i, v))
        });
        let forgotten = core
            .forgotten_limit()
            .inspect_err(|e| tracing::warn!("invalid forgotten limit: {e}"))
            .unwrap_or(clocky_core::store::DEFAULT_FORGOTTEN_LIMIT);
        Self {
            dark: settings::Settings::read(core).dark_theme,
            forgotten_minutes: u32::try_from(forgotten.as_secs() / 60).unwrap_or(FORGOTTEN_MAX),
            wizard,
            alarm_sound: core
                .setting(keys::ALARM_SOUND)
                .ok()
                .flatten()
                .unwrap_or_default(),
        }
    }

    /// "8h", "1h 30min" or "30min".
    pub fn forgotten_label(&self) -> String {
        hours_minutes(self.forgotten_minutes)
    }
}

/// Saves the theme.
pub fn set_theme(core: &mut AppCore, dark: bool) -> anyhow::Result<Applied> {
    settings::save_theme(core, dark)?;
    Ok(Applied::Theme(dark))
}

/// Moves the forgotten limit one step (`direction` < 0: shorter) and saves it.
pub fn step_forgotten(core: &mut AppCore, direction: i32) -> CoreResult<Applied> {
    let current = SettingsForm::load(core).forgotten_minutes;
    let next = stepped(
        current,
        direction,
        FORGOTTEN_STEP,
        FORGOTTEN_MIN,
        FORGOTTEN_MAX,
    );
    core.set_forgotten_limit(Duration::from_mins(u64::from(next)))?;
    Ok(Applied::Stored)
}

/// Moves wizard value `index` (0 = Foco … 3 = Fluxos) one step and saves it.
pub fn step_wizard(core: &mut AppCore, index: usize, direction: i32) -> CoreResult<Applied> {
    let (Some(key), Some(step), Some(info)) = (
        WIZARD_KEYS.get(index),
        WIZARD_STEPS.get(index),
        STEP_INFO.get(index),
    ) else {
        return Ok(Applied::Stored);
    };
    let current = SettingsForm::load(core).wizard[index];
    let next = stepped(current, direction, *step, info.min, info.max);
    core.set_setting(key, &next.to_string())?;
    Ok(Applied::Wizard)
}

/// Saves the default alarm sound (trimmed; empty = the built-in sound).
pub fn set_alarm_sound(core: &mut AppCore, path: &str) -> CoreResult<Applied> {
    core.set_setting(keys::ALARM_SOUND, path.trim())?;
    Ok(Applied::Stored)
}

/// `value` moved by one `step` toward `direction`, snapped to the step grid and kept in
/// `min..=max`: 52 + 5 gives 55, not 57.
fn stepped(value: u32, direction: i32, step: u32, min: u32, max: u32) -> u32 {
    let next = match direction.signum() {
        1 => (value / step + 1) * step,
        -1 => value.div_ceil(step).saturating_sub(1) * step,
        _ => value,
    };
    next.clamp(min, max)
}

fn clamp_wizard(index: usize, value: u32) -> u32 {
    STEP_INFO
        .get(index)
        .map_or(value, |info| value.clamp(info.min, info.max))
}

/// "8h", "1h 30min" or "30min".
fn hours_minutes(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}min"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m:02}min"),
    }
}

#[cfg(test)]
mod tests;
