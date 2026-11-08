//! Pomodoro and wizard actions.

use clocky_core::domain::{PomodoroPresetId, TaskId};

use super::TimerPanels;
use super::cards::Kind;
use super::frame_pomodoro::preset_config;
use super::state::{PomodoroPick, View};
use super::wizard::{Defaults, Wizard};

/// Store keys of the wizard defaults (decision 13: the settings only seed the wizard).
pub const DEFAULT_KEYS: [&str; 5] = [
    "pomodoro_focus_min",
    "pomodoro_short_min",
    "pomodoro_long_min",
    "pomodoro_rounds",
    "pomodoro_auto_start",
];

/// Pomodoro.
impl TimerPanels {
    pub(super) fn pomodoro_open_preset(&self, id: i32) {
        let Some(id) = super::convert::row_from_ui(id).map(PomodoroPresetId) else {
            return;
        };
        let task = {
            let handle = self.core.borrow();
            let presets = handle
                .tracker()
                .store()
                .pomodoro_presets()
                .unwrap_or_default();
            presets
                .into_iter()
                .find(|p| p.id == id)
                .and_then(|p| p.task)
        };
        self.open_draft(id, task);
    }

    fn open_draft(&self, id: PomodoroPresetId, task: Option<TaskId>) {
        {
            let mut state = self.state.borrow_mut();
            state.pomodoro = PomodoroPick::Draft(id);
            state.pomodoro_task = task;
            state.view = View::PomodoroTimer;
        }
        self.render();
    }

    pub(super) fn pomodoro_primary(&self) {
        let pick = self.state.borrow().pomodoro;
        match pick {
            PomodoroPick::Draft(id) => self.pomodoro_start(id),
            PomodoroPick::Active(_) => {
                if let Some(id) = self.picked(Kind::Pomodoro) {
                    self.toggle(id);
                }
            }
        }
    }

    fn pomodoro_start(&self, id: PomodoroPresetId) {
        let config = {
            let handle = self.core.borrow();
            let presets = handle
                .tracker()
                .store()
                .pomodoro_presets()
                .unwrap_or_default();
            presets.iter().find(|p| p.id == id).and_then(preset_config)
        };
        let Some(config) = config else {
            return;
        };
        self.forget_deleted_tasks();
        let task = self.state.borrow().pomodoro_task;
        let started = self.command("start pomodoro", |t, now| {
            t.start_pomodoro(task, config, Some(id), now)
        });
        if let Some(timer) = started {
            self.state.borrow_mut().pomodoro = PomodoroPick::Active(Some(timer));
        }
        self.render();
    }

    pub(super) fn pomodoro_stop(&self) {
        if let Some(id) = self.picked(Kind::Pomodoro) {
            self.stop(id);
        }
    }

    pub(super) fn pomodoro_skip(&self) {
        if let Some(id) = self.picked(Kind::Pomodoro) {
            self.command("skip phase", |t, now| t.skip(id, now));
            self.render();
        }
    }
}

/// Novo pomodoro.
impl TimerPanels {
    /// Reads the wizard defaults from the store settings (constants when unset or invalid).
    pub(super) fn load_wizard_defaults(&self) {
        let values: Vec<Option<String>> = {
            let handle = self.core.borrow();
            let store = handle.tracker().store();
            DEFAULT_KEYS
                .iter()
                .map(|key| store.setting(key).ok().flatten())
                .collect()
        };
        let base = Defaults::default();
        let num = |i: usize, fallback: u32| {
            values[i]
                .as_deref()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(fallback)
        };
        let defaults = Defaults {
            focus: num(0, base.focus),
            short: num(1, base.short),
            long: num(2, base.long),
            rounds: num(3, base.rounds),
            auto_start: values[4]
                .as_deref()
                .map_or(base.auto_start, |v| v == "true" || v == "1"),
        };
        let mut state = self.state.borrow_mut();
        state.wizard = Wizard::new(defaults);
        state.wizard_defaults = defaults;
    }

    pub(super) fn wizard_open(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.wizard = Wizard::new(state.wizard_defaults);
            state.view = View::Wizard;
        }
        self.render();
    }

    pub(super) fn wizard_edit(&self, edit: impl FnOnce(&mut Wizard)) {
        edit(&mut self.state.borrow_mut().wizard);
        self.render();
    }

    pub(super) fn wizard_back(&self) {
        let moved = self.state.borrow_mut().wizard.back();
        if moved {
            self.render();
        } else {
            self.set_view(View::PomodoroList);
        }
    }

    /// "Próximo", or "Criar pomodoro" on the last step: saves the preset and opens its timer.
    pub(super) fn wizard_next(&self) {
        let moved = self.state.borrow_mut().wizard.next();
        if moved {
            self.render();
            return;
        }
        self.forget_deleted_tasks();
        let wizard = self.state.borrow().wizard.clone();
        let name = wizard.final_name();
        let created = self.command("create pomodoro preset", |t, _| {
            Ok(t.store_mut()
                .create_pomodoro_preset(&wizard.preset(&name))?)
        });
        match created {
            Some(id) => self.open_draft(id, wizard.task),
            None => self.render(),
        }
    }
}
