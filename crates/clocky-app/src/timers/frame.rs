//! Builds what the panel shows from the tracker and the panel state, as plain Slint values.
//! Nothing here touches a window: the caller applies the frame once every borrow is released.

use clocky_core::domain::TaskId;
use clocky_core::engine::timers::Timer;
use clocky_core::store::Store;
use clocky_core::time::Timestamp;
use clocky_core::tracker::Tracker;
use slint::SharedString;

use super::cards::Kind;
use super::convert::{color, count_ratio, fraction, row_to_ui};
use super::format;
use super::names::{Names, TaskLabel};
use super::state::{State, View, pick, pick_countdown};
use crate::ui;

/// Everything the current view needs.
pub struct Frame {
    pub title: SharedString,
    pub subtitle: SharedString,
    pub icon_kind: &'static str,
    pub notice: SharedString,
    pub body: Body,
    pub forgotten: Option<SharedString>,
}

pub enum Body {
    None,
    Stopwatch(StopwatchFrame),
    CountdownConfig(CountdownConfigFrame),
    CountdownRun(CountdownRunFrame),
    PomodoroList(Vec<ui::PomodoroCard>),
    PomodoroTimer(super::frame_pomodoro::PomodoroTimerFrame),
    Wizard(super::frame_pomodoro::WizardFrame),
    Overview(Vec<ui::ActiveCard>),
}

pub struct StopwatchFrame {
    pub state: i32,
    pub time: SharedString,
    pub task: ui::TaskLine,
    pub subtasks: Vec<ui::SubtaskRow>,
    pub progress_label: SharedString,
    pub progress: f32,
}

pub struct CountdownConfigFrame {
    pub fields: [SharedString; 3],
    pub can_start: bool,
    pub task: ui::TaskLine,
    pub presets: Vec<ui::PresetRow>,
}

pub struct CountdownRunFrame {
    /// 1 = running, 2 = paused, 3 = ringing.
    pub state: i32,
    pub status: SharedString,
    pub time: SharedString,
    pub caption: SharedString,
    pub progress: f32,
    pub task: ui::TaskLine,
}

/// The read side the frame is built from.
pub struct Source<'a> {
    pub tracker: &'a Tracker,
    pub state: &'a State,
    pub names: &'a Names,
    pub now: Timestamp,
}

impl Source<'_> {
    pub fn store(&self) -> &Store {
        self.tracker.store()
    }
}

/// The task line of a timer or draft.
pub fn task_line(label: Option<&TaskLabel>) -> ui::TaskLine {
    match label {
        Some(label) => ui::TaskLine {
            linked: true,
            project: label.project.as_str().into(),
            task: label.task.as_str().into(),
            color: color(&label.color),
        },
        None => ui::TaskLine::default(),
    }
}

pub fn build(src: &Source<'_>) -> Frame {
    let view = src.state.view;
    let (title, subtitle, icon_kind) = header(view);
    let mut frame = Frame {
        title: title.into(),
        subtitle: subtitle.into(),
        icon_kind,
        notice: src.state.notice.as_str().into(),
        body: Body::None,
        forgotten: forgotten_text(src),
    };
    frame.body = match view {
        View::Hidden => Body::None,
        View::Stopwatch => Body::Stopwatch(stopwatch(src)),
        View::CountdownConfig => Body::CountdownConfig(countdown_config(src)),
        View::CountdownRun => Body::CountdownRun(countdown_run(src)),
        View::PomodoroList => Body::PomodoroList(super::frame_pomodoro::list(src)),
        View::PomodoroTimer => super::frame_pomodoro::timer(src, &mut frame),
        View::Wizard => super::frame_pomodoro::wizard(src, &mut frame),
        View::Overview => super::frame_pomodoro::overview(src, &mut frame),
    };
    frame
}

fn header(view: View) -> (&'static str, &'static str, &'static str) {
    match view {
        View::Hidden => ("", "", ""),
        View::Stopwatch => ("Cronômetro", "", "stopwatch"),
        View::CountdownConfig | View::CountdownRun => ("Temporizador", "", "countdown"),
        View::PomodoroList | View::PomodoroTimer => ("Pomodoro", "", "pomodoro"),
        View::Wizard => ("Novo pomodoro", "", "pomodoro"),
        View::Overview => ("Ativos", "", "overview"),
    }
}

fn forgotten_text(src: &Source<'_>) -> Option<SharedString> {
    let id = src.state.forgotten?;
    let active = src.tracker.timers().get(id)?;
    let Timer::Stopwatch(sw) = &active.timer else {
        return None;
    };
    let elapsed = format::duration_short(sw.elapsed(src.now));
    Some(
        format!(
            "O cronômetro está rodando há {elapsed}. Registrar todo esse tempo ou descartar a \
             sessão?"
        )
        .into(),
    )
}

fn stopwatch(src: &Source<'_>) -> StopwatchFrame {
    let timers = src.tracker.timers();
    let active = pick(timers, Kind::Stopwatch, src.state.stopwatch).and_then(|id| timers.get(id));
    let (state, time, task) = match active {
        Some(active) => match &active.timer {
            Timer::Stopwatch(sw) => (
                if sw.is_running() { 1 } else { 2 },
                format::elapsed_label(sw.elapsed(src.now)),
                active.task,
            ),
            _ => (0, format::hh_mm_ss(0), active.task),
        },
        None => (0, format::hh_mm_ss(0), src.state.stopwatch_task),
    };
    let label = src.names.task(task);
    let (subtasks, done) = subtask_rows(src.store(), task.filter(|_| label.is_some()));
    let total = subtasks.len();
    StopwatchFrame {
        state,
        time: time.into(),
        task: task_line(label),
        progress_label: format!("{done}/{total} concluídas").into(),
        progress: count_ratio(done, total),
        subtasks,
    }
}

fn subtask_rows(store: &Store, task: Option<TaskId>) -> (Vec<ui::SubtaskRow>, usize) {
    let Some(task) = task else {
        return (Vec::new(), 0);
    };
    let subtasks = store.subtasks(task).unwrap_or_else(|e| {
        tracing::warn!("cannot read the subtasks of task {}: {e}", task.0);
        Vec::new()
    });
    let done = subtasks.iter().filter(|s| s.completed_at.is_some()).count();
    let rows = subtasks
        .into_iter()
        .map(|s| ui::SubtaskRow {
            id: row_to_ui(s.id.0),
            name: s.name.into(),
            done: s.completed_at.is_some(),
        })
        .collect();
    (rows, done)
}

fn countdown_config(src: &Source<'_>) -> CountdownConfigFrame {
    let hms = src.state.hms;
    let presets = src
        .names
        .timer_presets
        .iter()
        .map(|p| ui::PresetRow {
            id: row_to_ui(p.id.0),
            name: p.name.as_str().into(),
            duration: format::duration_short(p.duration).into(),
            task: task_line(src.names.task(p.task)),
        })
        .collect();
    CountdownConfigFrame {
        fields: hms.texts().map(SharedString::from),
        can_start: !hms.is_zero(),
        task: task_line(src.names.task(src.state.countdown_task)),
        presets,
    }
}

fn countdown_run(src: &Source<'_>) -> CountdownRunFrame {
    let timers = src.tracker.timers();
    let picked = pick_countdown(timers, &src.state.ringing, src.state.countdown);
    if let Some(ringing) = picked.and_then(|id| src.state.ringing(id)) {
        return CountdownRunFrame {
            state: 3,
            status: "Tempo esgotado".into(),
            time: "00:00".into(),
            caption: format!(
                "{} · terminou às {}",
                ringing.name,
                format::local_hh_mm(ringing.at)
            )
            .into(),
            progress: 1.0,
            task: task_line(src.names.task(ringing.task)),
        };
    }
    let active = picked.and_then(|id| timers.get(id));
    let Some((active, Timer::Countdown(cd))) = active.map(|a| (a, &a.timer)) else {
        return CountdownRunFrame {
            state: 1,
            status: SharedString::new(),
            time: "00:00".into(),
            caption: SharedString::new(),
            progress: 0.0,
            task: ui::TaskLine::default(),
        };
    };
    let remaining = cd.remaining(src.now);
    let name = src.names.info(active).name;
    let running = cd.is_running();
    let caption = if running {
        format!(
            "{name} · termina às {}",
            format::local_hh_mm(src.now.plus(remaining))
        )
    } else {
        format!("{name} · pausado")
    };
    CountdownRunFrame {
        state: if running { 1 } else { 2 },
        status: if running { "RODANDO" } else { "PAUSADO" }.into(),
        time: format::remaining_label(remaining).into(),
        caption: caption.into(),
        progress: 1.0 - fraction(remaining, cd.target()),
        task: task_line(src.names.task(active.task)),
    }
}
