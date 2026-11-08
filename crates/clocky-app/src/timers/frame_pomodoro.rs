//! Frames of the pomodoro list, timer and wizard, and of the Ativos list.

use clocky_core::engine::pomodoro::{Phase, PomodoroConfig};
use clocky_core::engine::timers::Timer;
use clocky_core::store::PomodoroPreset;
use slint::{ModelRc, SharedString, VecModel};

use super::cards::{self, Card, Kind, State as CardState};
use super::convert::{row_to_ui, timer_to_ui};
use super::format;
use super::frame::{Body, Frame, Source, task_line};
use super::pomodoro_view::{self, PomodoroView};
use super::segments::segment_bar;
use super::state::{PomodoroPick, pick};
use super::wizard::STEPS;
use crate::ui;

pub struct PomodoroTimerFrame {
    pub view: PomodoroView,
    pub arc: SharedString,
    pub head: (f32, f32),
    pub dots: Vec<i32>,
    pub task: ui::TaskLine,
}

pub struct WizardFrame {
    pub step: i32,
    pub title: SharedString,
    pub help: SharedString,
    pub unit: SharedString,
    pub value: i32,
    pub shortcuts: Vec<i32>,
    pub summary: SharedString,
    pub placeholder: SharedString,
    pub auto_start: bool,
    pub task: ui::TaskLine,
}

/// The configuration a saved pomodoro starts with.
pub fn preset_config(preset: &PomodoroPreset) -> Option<PomodoroConfig> {
    PomodoroConfig::new(
        preset.focus,
        preset.short_break,
        preset.long_break,
        preset.focus_per_round,
        preset.auto_start,
    )
    .inspect_err(|e| tracing::warn!("invalid pomodoro preset {}: {e}", preset.id.0))
    .ok()
}

pub fn list(src: &Source<'_>) -> Vec<ui::PomodoroCard> {
    src.names
        .pomodoro_presets
        .iter()
        .map(|p| {
            let parts: Vec<ui::BarPart> =
                segment_bar(p.focus, p.short_break, p.long_break, p.focus_per_round)
                    .into_iter()
                    .map(|part| ui::BarPart {
                        fraction: part.fraction,
                        focus: part.focus,
                    })
                    .collect();
            ui::PomodoroCard {
                id: row_to_ui(p.id.0),
                name: p.name.as_str().into(),
                summary: format::pomodoro_summary(
                    p.focus,
                    p.short_break,
                    p.long_break,
                    p.focus_per_round,
                )
                .into(),
                parts: ModelRc::new(VecModel::from(parts)),
                task: task_line(src.names.task(p.task)),
            }
        })
        .collect()
}

fn durations(config: &PomodoroConfig) -> String {
    format::pomodoro_durations(
        config.duration(Phase::Focus),
        config.duration(Phase::ShortBreak),
        config.duration(Phase::LongBreak),
    )
}

/// The pomodoro timer: the picked active pomodoro, or a saved one not started yet.
pub fn timer(src: &Source<'_>, frame: &mut Frame) -> Body {
    let timers = src.tracker.timers();
    let (view, config, name, task) = match src.state.pomodoro {
        PomodoroPick::Active(selected) => {
            let Some(active) = pick(timers, Kind::Pomodoro, selected).and_then(|id| timers.get(id))
            else {
                return Body::PomodoroList(list(src));
            };
            let Timer::Pomodoro(p) = &active.timer else {
                return Body::PomodoroList(list(src));
            };
            let name = src.names.info(active).name;
            (
                pomodoro_view::running(p, src.now),
                p.config().clone(),
                name,
                active.task,
            )
        }
        PomodoroPick::Draft(id) => {
            let Some(preset) = src.names.pomodoro_preset(id) else {
                return Body::PomodoroList(list(src));
            };
            let Some(config) = preset_config(preset) else {
                return Body::PomodoroList(list(src));
            };
            (
                pomodoro_view::idle(&config),
                config,
                preset.name.clone(),
                src.state.pomodoro_task,
            )
        }
    };
    frame.title = name.into();
    frame.subtitle = durations(&config).into();
    let (x, y) = pomodoro_view::arc_head(view.progress);
    Body::PomodoroTimer(PomodoroTimerFrame {
        arc: pomodoro_view::arc_path(view.progress).into(),
        head: (to_f32(x), to_f32(y)),
        dots: view.dots.iter().map(|d| d.as_index()).collect(),
        task: task_line(src.names.task(task)),
        view,
    })
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "ring coordinates are within 0..=220"
)]
fn to_f32(value: f64) -> f32 {
    value as f32
}

pub fn wizard(src: &Source<'_>, frame: &mut Frame) -> Body {
    let wizard = &src.state.wizard;
    let info = wizard.info();
    frame.subtitle = format!("Etapa {} de {STEPS}", wizard.step + 1).into();
    let as_i32 = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    Body::Wizard(WizardFrame {
        step: i32::try_from(wizard.step).unwrap_or(0),
        title: info.title.into(),
        help: info.help.into(),
        unit: info.unit.into(),
        value: as_i32(wizard.value()),
        shortcuts: info.shortcuts.iter().map(|v| as_i32(*v)).collect(),
        summary: wizard.summary().into(),
        placeholder: wizard.default_name().into(),
        auto_start: wizard.auto_start,
        task: task_line(src.names.task(wizard.task)),
    })
}

/// Cards of every active timer and ringing countdown, by urgency.
pub fn cards(src: &Source<'_>) -> Vec<Card> {
    let mut cards: Vec<Card> = src
        .tracker
        .timers()
        .iter()
        .map(|active| cards::card(active, src.now, src.names.preset_name(active.preset)))
        .chain(
            src.state
                .ringing
                .iter()
                .map(|r| cards::ringing_card(r, &format::local_hh_mm(r.at))),
        )
        .collect();
    cards::sort_by_urgency(&mut cards);
    cards
}

pub fn overview(src: &Source<'_>, frame: &mut Frame) -> Body {
    let cards = cards(src);
    frame.subtitle = format::active_count(cards.len(), 0).into();
    Body::Overview(
        cards
            .into_iter()
            .map(|card| ui::ActiveCard {
                id: timer_to_ui(card.id),
                kind: card.kind.as_str().into(),
                name: card.name.into(),
                status: card.status.into(),
                time: card.time.into(),
                has_progress: card.progress.is_some(),
                progress: card.progress.unwrap_or(0.0),
                running: card.state == CardState::Running,
                waiting: card.state == CardState::Waiting,
                ringing: card.state == CardState::Ringing,
                task: task_line(src.names.task(card.task)),
            })
            .collect(),
    )
}
