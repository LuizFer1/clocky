//! M7b: the Configurações page of the manage window. Every control saves right away; the app
//! hook applies what the rest of the app needs (theme, wizard defaults).
// Slint callbacks hand their arguments over by value.
#![allow(clippy::needless_pass_by_value)]

use std::fmt::Display;
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::{Manage, Section};
use crate::core::AppCore;
use crate::settings_view::{self, Applied, FORGOTTEN_MAX, FORGOTTEN_MIN, SettingsForm};
use crate::timers::STEP_INFO;
use crate::{autostart, format, ui};

/// What the switch says under "Iniciar com o sistema".
const AUTOSTART_HELP: &str = if cfg!(windows) {
    "Abre o Clocky na bandeja quando você entra no Windows."
} else if cfg!(target_os = "macos") {
    "Abre o Clocky quando você entra no macOS."
} else {
    "Abre o Clocky quando você entra na sessão."
};

impl Manage {
    pub(super) fn connect_settings(&self) {
        let settings = self.settings_state();
        settings.on_set_theme(self.handler1(|m, dark| {
            m.change_setting("save the theme", |c| settings_view::set_theme(c, dark));
        }));
        settings.on_step_forgotten(self.handler1(|m, direction| {
            m.change_setting("save the forgotten limit", |c| {
                settings_view::step_forgotten(c, direction)
            });
        }));
        settings.on_step_wizard(self.handler2(|m, index, direction| {
            let index = usize::try_from(index).unwrap_or(usize::MAX);
            m.change_setting("save a wizard default", |c| {
                settings_view::step_wizard(c, index, direction)
            });
        }));
        settings.on_alarm_sound_edited(self.handler1(|m, path: SharedString| {
            m.change_setting("save the alarm sound", |c| {
                settings_view::set_alarm_sound(c, &path)
            });
        }));
        settings.on_set_autostart(self.handler1(Self::set_autostart));
        self.state()
            .on_open_settings(self.handler0(Self::open_settings));
    }

    fn settings_state(&self) -> ui::SettingsState<'_> {
        self.ui.global::<ui::SettingsState>()
    }

    pub fn open_settings(&self) {
        self.section.set(Section::Settings);
        self.settings_state()
            .set_autostart_error(SharedString::new());
        self.refresh();
    }

    /// The real autostart entry (the default is in memory only, see [`autostart::Fake`]).
    pub fn set_autostart_backend(&self, backend: Box<dyn autostart::Autostart>) {
        *self.autostart.borrow_mut() = backend;
    }

    /// What the app does after a change (switch the theme, reload the wizard defaults).
    pub fn set_on_settings(&self, hook: impl Fn(Applied) + 'static) {
        *self.on_settings.borrow_mut() = Some(Box::new(hook));
    }

    /// Saves one change (short core borrow), runs the app hook and shows the new values.
    fn change_setting<E: Display>(
        &self,
        what: &str,
        change: impl FnOnce(&mut AppCore) -> Result<Applied, E>,
    ) {
        let result = change(&mut self.core.borrow_mut());
        match result {
            Ok(applied) => {
                // Taken out while it runs: the hook touches this window (theme).
                let hook = self.on_settings.borrow_mut().take();
                if let Some(hook) = hook {
                    hook(applied);
                    self.on_settings.borrow_mut().get_or_insert(hook);
                }
            }
            Err(e) => tracing::warn!("failed to {what}: {e}"),
        }
        self.refresh();
    }

    fn set_autostart(&self, on: bool) {
        let result = autostart::set(self.autostart.borrow().as_ref(), on);
        let error = match result {
            Ok(_) => String::new(),
            Err(e) => {
                tracing::warn!("failed to change the start with the system: {e}");
                format!("Não deu para mudar: {e}")
            }
        };
        self.settings_state().set_autostart_error(error.into());
        self.refresh();
    }

    /// Fills the Configurações page and the header. Runs after every refresh while it is open.
    pub(super) fn apply_settings(&self) {
        let form = SettingsForm::load(&self.core.borrow());
        let s = self.settings_state();
        s.set_dark(form.dark);
        s.set_forgotten(form.forgotten_label().into());
        s.set_forgotten_at_min(form.forgotten_minutes <= FORGOTTEN_MIN);
        s.set_forgotten_at_max(form.forgotten_minutes >= FORGOTTEN_MAX);
        let values: Vec<i32> = form
            .wizard
            .iter()
            .map(|v| i32::try_from(*v).unwrap_or(i32::MAX))
            .collect();
        let at_min: Vec<bool> = form
            .wizard
            .iter()
            .zip(&STEP_INFO)
            .map(|(v, info)| *v <= info.min)
            .collect();
        let at_max: Vec<bool> = form
            .wizard
            .iter()
            .zip(&STEP_INFO)
            .map(|(v, info)| *v >= info.max)
            .collect();
        s.set_wizard(ModelRc::new(VecModel::from(values)));
        s.set_wizard_at_min(ModelRc::new(VecModel::from(at_min)));
        s.set_wizard_at_max(ModelRc::new(VecModel::from(at_max)));
        let [focus, short, long, rounds] = form.wizard;
        let min = |m: u32| Duration::from_mins(u64::from(m));
        s.set_wizard_summary(
            format::pomodoro_summary(min(focus), min(short), min(long), rounds).into(),
        );
        // Not while the user types in the field (the text would jump).
        if s.get_alarm_sound().trim() != form.alarm_sound {
            s.set_alarm_sound(form.alarm_sound.as_str().into());
        }
        let enabled = self.autostart.borrow().is_enabled();
        s.set_autostart_available(enabled.is_ok());
        s.set_autostart(enabled.unwrap_or(false));
        s.set_autostart_help(AUTOSTART_HELP.into());
        let state = self.state();
        state.set_page(5);
        state.set_crumb("Configurações".into());
        state.set_heading("Configurações".into());
        state.set_subheading("Tema, valores iniciais e o início com o sistema".into());
        state.set_has_heading_color(false);
    }
}
