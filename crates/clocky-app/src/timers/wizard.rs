//! The "Novo pomodoro" wizard (screens 44–47): four steps (Foco, Pausa, Longa, Fluxos), each
//! with a value, − / + and shortcuts. Step 3 also holds auto-start and step 4 the name and task.

use std::time::Duration;

use clocky_core::domain::TaskId;
use clocky_core::store::NewPomodoroPreset;

use super::format;

/// Number of steps.
pub const STEPS: usize = 4;

/// Text and limits of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub label: &'static str,
    pub title: &'static str,
    pub help: &'static str,
    pub unit: &'static str,
    pub min: u32,
    pub max: u32,
    pub shortcuts: [u32; 4],
}

pub const STEP_INFO: [Step; STEPS] = [
    Step {
        label: "Foco",
        title: "Tempo de foco",
        help: "Quanto dura cada bloco de trabalho.",
        unit: "min",
        min: 1,
        max: 180,
        shortcuts: [15, 25, 45, 50],
    },
    Step {
        label: "Pausa",
        title: "Pausa curta",
        help: "O descanso entre um foco e outro.",
        unit: "min",
        min: 1,
        max: 60,
        shortcuts: [3, 5, 10, 15],
    },
    Step {
        label: "Longa",
        title: "Pausa longa",
        help: "O descanso maior, ao fim de cada ciclo.",
        unit: "min",
        min: 1,
        max: 120,
        shortcuts: [15, 20, 30, 45],
    },
    Step {
        label: "Fluxos",
        title: "Fluxos até a pausa longa",
        help: "Quantos focos você faz antes de ganhar a pausa longa.",
        unit: "focos",
        min: 1,
        max: 12,
        shortcuts: [2, 3, 4, 6],
    },
];

/// Initial values of the wizard (the global settings only seed it, decision 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Defaults {
    pub focus: u32,
    pub short: u32,
    pub long: u32,
    pub rounds: u32,
    pub auto_start: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            focus: 25,
            short: 5,
            long: 15,
            rounds: 4,
            auto_start: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wizard {
    /// 0-based step.
    pub step: usize,
    /// Focus, short break, long break (minutes) and focus phases per round.
    pub values: [u32; STEPS],
    pub auto_start: bool,
    pub name: String,
    pub task: Option<TaskId>,
}

impl Wizard {
    pub fn new(defaults: Defaults) -> Self {
        let mut wizard = Self {
            step: 0,
            values: [
                defaults.focus,
                defaults.short,
                defaults.long,
                defaults.rounds,
            ],
            auto_start: defaults.auto_start,
            name: String::new(),
            task: None,
        };
        for step in 0..STEPS {
            wizard.values[step] = clamp(step, wizard.values[step]);
        }
        wizard
    }

    pub fn info(&self) -> &'static Step {
        &STEP_INFO[self.step.min(STEPS - 1)]
    }

    pub fn value(&self) -> u32 {
        self.values[self.step.min(STEPS - 1)]
    }

    /// Sets the value of the current step, clamped to its limits.
    pub fn set_value(&mut self, value: u32) {
        let step = self.step.min(STEPS - 1);
        self.values[step] = clamp(step, value);
    }

    /// The − / + buttons.
    pub fn bump(&mut self, up: bool) {
        let value = self.value();
        self.set_value(if up {
            value.saturating_add(1)
        } else {
            value.saturating_sub(1)
        });
    }

    /// "Próximo": `false` on the last step (the footer shows "Criar pomodoro" there).
    pub fn next(&mut self) -> bool {
        if self.step + 1 < STEPS {
            self.step += 1;
            true
        } else {
            false
        }
    }

    /// "Voltar": `false` on the first step (the footer shows "Cancelar" there).
    pub fn back(&mut self) -> bool {
        if self.step > 0 {
            self.step -= 1;
            true
        } else {
            false
        }
    }

    fn duration(&self, step: usize) -> Duration {
        Duration::from_mins(u64::from(self.values[step]))
    }

    /// "25 · 5 · 15 min · 4×".
    pub fn summary(&self) -> String {
        format::pomodoro_summary(
            self.duration(0),
            self.duration(1),
            self.duration(2),
            self.values[3],
        )
    }

    /// "Personalizado 25/5", used when the name is left empty.
    pub fn default_name(&self) -> String {
        format!("Personalizado {}/{}", self.values[0], self.values[1])
    }

    /// The name that will be saved.
    pub fn final_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            self.default_name()
        } else {
            name.to_owned()
        }
    }

    /// The preset row to create, borrowing `name` (see [`Wizard::final_name`]).
    pub fn preset<'a>(&self, name: &'a str) -> NewPomodoroPreset<'a> {
        NewPomodoroPreset {
            name,
            focus: self.duration(0),
            short_break: self.duration(1),
            long_break: self.duration(2),
            focus_per_round: self.values[3],
            auto_start: self.auto_start,
            task: self.task,
        }
    }
}

fn clamp(step: usize, value: u32) -> u32 {
    let info = &STEP_INFO[step];
    value.clamp(info.min, info.max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_on_focus_with_the_defaults() {
        let w = Wizard::new(Defaults::default());
        assert_eq!((w.step, w.value()), (0, 25));
        assert_eq!(w.summary(), "25 · 5 · 15 min · 4×");
        assert_eq!(w.info().title, "Tempo de foco");
    }

    #[test]
    fn values_are_clamped_per_step() {
        let mut w = Wizard::new(Defaults {
            focus: 0,
            rounds: 99,
            ..Defaults::default()
        });
        assert_eq!(w.values, [1, 5, 15, 12]);
        w.bump(false);
        assert_eq!(w.value(), 1);
        w.set_value(500);
        assert_eq!(w.value(), 180);
    }

    #[test]
    fn steps_move_forward_and_back() {
        let mut w = Wizard::new(Defaults::default());
        assert!(!w.back());
        assert!(w.next() && w.next() && w.next());
        assert!(!w.next());
        assert_eq!(w.info().unit, "focos");
        w.bump(true);
        assert_eq!(w.summary(), "25 · 5 · 15 min · 5×");
        assert!(w.back());
        assert_eq!(w.info().label, "Longa");
    }

    #[test]
    fn empty_name_falls_back_to_personalizado() {
        let mut w = Wizard::new(Defaults::default());
        assert_eq!(w.final_name(), "Personalizado 25/5");
        w.name = "  Deep work ".into();
        assert_eq!(w.final_name(), "Deep work");
    }

    #[test]
    fn preset_copies_every_value() {
        let mut w = Wizard::new(Defaults::default());
        w.values = [50, 10, 30, 3];
        w.auto_start = true;
        w.task = Some(TaskId(4));
        let p = w.preset("x");
        assert_eq!(p.focus, Duration::from_mins(50));
        assert_eq!(p.short_break, Duration::from_mins(10));
        assert_eq!(p.long_break, Duration::from_mins(30));
        assert_eq!((p.focus_per_round, p.auto_start), (3, true));
        assert_eq!(p.task, Some(TaskId(4)));
    }
}
