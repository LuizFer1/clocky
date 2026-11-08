//! Names the panels read from the store: task labels (`projeto › tarefa`), preset names and the
//! name/task of each timer for the alerts. Read failures are logged and show as missing names.

use std::collections::HashMap;

use clocky_core::domain::{PomodoroPresetId, PresetRef, ProjectId, TaskId, TimerPresetId};
use clocky_core::engine::timers::ActiveTimer;
use clocky_core::store::{PomodoroPreset, Store, StoreError, TimerPreset};

use super::alerts::TimerInfo;
use super::cards::Kind;

/// Shown for a task without a project.
const NO_PROJECT: &str = "Sem projeto";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLabel {
    pub project: String,
    pub task: String,
    /// `#rrggbb`.
    pub color: String,
}

impl TaskLabel {
    /// "connect › testes".
    pub fn text(&self) -> String {
        format!("{} › {}", self.project, self.task)
    }
}

#[derive(Debug, Default)]
pub struct Names {
    pub timer_presets: Vec<TimerPreset>,
    pub pomodoro_presets: Vec<PomodoroPreset>,
    tasks: HashMap<TaskId, TaskLabel>,
}

/// Logs a failed read and gives the default.
fn or_log<T: Default>(what: &str, result: Result<T, StoreError>) -> T {
    result.unwrap_or_else(|e| {
        tracing::warn!("cannot read {what}: {e}");
        T::default()
    })
}

impl Names {
    /// Reads the presets and the labels of `tasks` plus the tasks of the presets.
    pub fn load(store: &Store, tasks: impl IntoIterator<Item = TaskId>) -> Self {
        let timer_presets = or_log("timer presets", store.timer_presets());
        let pomodoro_presets = or_log("pomodoro presets", store.pomodoro_presets());
        let projects: HashMap<ProjectId, (String, String)> =
            or_log("projects", store.projects(true))
                .into_iter()
                .map(|p| (p.id, (p.name, p.color)))
                .collect();
        let wanted = tasks
            .into_iter()
            .chain(timer_presets.iter().filter_map(|p| p.task))
            .chain(pomodoro_presets.iter().filter_map(|p| p.task));
        let mut labels = HashMap::new();
        for id in wanted {
            if labels.contains_key(&id) {
                continue;
            }
            if let Some(label) = task_label(store, &projects, id) {
                labels.insert(id, label);
            }
        }
        Self {
            timer_presets,
            pomodoro_presets,
            tasks: labels,
        }
    }

    pub fn task(&self, id: Option<TaskId>) -> Option<&TaskLabel> {
        id.and_then(|id| self.tasks.get(&id))
    }

    pub fn timer_preset(&self, id: TimerPresetId) -> Option<&TimerPreset> {
        self.timer_presets.iter().find(|p| p.id == id)
    }

    pub fn pomodoro_preset(&self, id: PomodoroPresetId) -> Option<&PomodoroPreset> {
        self.pomodoro_presets.iter().find(|p| p.id == id)
    }

    pub fn preset_name(&self, preset: Option<PresetRef>) -> Option<&str> {
        match preset? {
            PresetRef::Timer(id) => self.timer_preset(id).map(|p| p.name.as_str()),
            PresetRef::Pomodoro(id) => self.pomodoro_preset(id).map(|p| p.name.as_str()),
        }
    }

    /// Name and task of a timer for its alerts.
    pub fn info(&self, active: &ActiveTimer) -> TimerInfo {
        let preset = self.preset_name(active.preset);
        let name = match (Kind::of(&active.timer), preset) {
            (Kind::Stopwatch, _) => "Cronômetro".to_owned(),
            (_, Some(name)) => name.to_owned(),
            (Kind::Countdown, None) => "Temporizador".to_owned(),
            (Kind::Pomodoro, None) => "Pomodoro".to_owned(),
        };
        TimerInfo {
            name,
            task: self.task(active.task).map(TaskLabel::text),
            task_id: active.task,
        }
    }
}

fn task_label(
    store: &Store,
    projects: &HashMap<ProjectId, (String, String)>,
    id: TaskId,
) -> Option<TaskLabel> {
    let task = match store.task(id) {
        Ok(task) => task,
        Err(e) => {
            tracing::warn!("cannot read task {}: {e}", id.0);
            return None;
        }
    };
    let (project, color) = task
        .project
        .and_then(|p| projects.get(&p).cloned())
        .unwrap_or_else(|| (NO_PROJECT.to_owned(), String::new()));
    Some(TaskLabel {
        project,
        task: task.name,
        color,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use clocky_core::domain::TimerId;
    use clocky_core::engine::countdown::Countdown;
    use clocky_core::engine::timers::Timer;
    use clocky_core::time::Timestamp;

    use super::*;

    #[test]
    fn labels_join_project_and_task() {
        let t0 = Timestamp::from_millis(0);
        let mut store = Store::open_in_memory().unwrap();
        let p = store.create_project("connect", "#3d8fd6", t0).unwrap();
        let t = store.create_task(Some(p), "testes", t0).unwrap();
        let loose = store.create_task(None, "solta", t0).unwrap();
        let preset = store
            .create_timer_preset("pipeline", Duration::from_mins(12), Some(t))
            .unwrap();
        let names = Names::load(&store, [loose]);
        assert_eq!(names.task(Some(t)).unwrap().text(), "connect › testes");
        assert_eq!(names.task(Some(t)).unwrap().color, "#3d8fd6");
        assert_eq!(
            names.task(Some(loose)).unwrap().text(),
            "Sem projeto › solta"
        );
        let active = ActiveTimer::new(
            TimerId(1),
            Some(t),
            Some(PresetRef::Timer(preset)),
            Timer::Countdown(Countdown::start(t0, Duration::from_mins(12))),
        );
        let info = names.info(&active);
        assert_eq!(info.name, "pipeline");
        assert_eq!(info.task.as_deref(), Some("connect › testes"));
    }

    #[test]
    fn missing_tasks_have_no_label() {
        let store = Store::open_in_memory().unwrap();
        let names = Names::load(&store, [TaskId(42)]);
        assert!(names.task(Some(TaskId(42))).is_none());
        assert!(names.task(None).is_none());
    }
}
