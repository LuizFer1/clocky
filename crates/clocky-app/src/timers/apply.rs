//! Pushes a [`Frame`] to the `TimerUi` global. Lists live in models created once; each frame
//! only rewrites the rows that changed, so the repeaters keep their items between ticks.

use std::rc::Rc;

use slint::{ComponentHandle, Model, VecModel};

use super::frame::{Body, CountdownConfigFrame, CountdownRunFrame, Frame, StopwatchFrame};
use super::frame_pomodoro::{PomodoroTimerFrame, WizardFrame};
use crate::ui;

pub struct Models {
    subtasks: Rc<VecModel<ui::TimerSubtaskRow>>,
    presets: Rc<VecModel<ui::PresetRow>>,
    pomodoros: Rc<VecModel<ui::PomodoroCard>>,
    dots: Rc<VecModel<i32>>,
    shortcuts: Rc<VecModel<i32>>,
    cards: Rc<VecModel<ui::ActiveCard>>,
}

impl Models {
    /// Creates the models and hands them to the global.
    pub fn install(global: &ui::TimerUi<'_>) -> Self {
        let models = Self {
            subtasks: Rc::new(VecModel::default()),
            presets: Rc::new(VecModel::default()),
            pomodoros: Rc::new(VecModel::default()),
            dots: Rc::new(VecModel::default()),
            shortcuts: Rc::new(VecModel::default()),
            cards: Rc::new(VecModel::default()),
        };
        global.set_sw_subtasks(models.subtasks.clone().into());
        global.set_cd_presets(models.presets.clone().into());
        global.set_pomo_presets(models.pomodoros.clone().into());
        global.set_pomo_dots(models.dots.clone().into());
        global.set_wz_shortcuts(models.shortcuts.clone().into());
        global.set_cards(models.cards.clone().into());
        models
    }
}

/// Replaces the rows of `model` with `rows`, touching only the ones that differ.
pub fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    sync_by(model, rows, |a, b| a == b);
}

/// [`sync`] with the row equality given: a row holding a nested model compares it by contents
/// (a `ModelRc` equals only itself, so a rebuilt one would always look changed).
fn sync_by<T: Clone + 'static>(model: &VecModel<T>, rows: Vec<T>, same: impl Fn(&T, &T) -> bool) {
    let len = rows.len();
    for (i, row) in rows.into_iter().enumerate() {
        if i < model.row_count() {
            if !model.row_data(i).is_some_and(|old| same(&old, &row)) {
                model.set_row_data(i, row);
            }
        } else {
            model.push(row);
        }
    }
    while model.row_count() > len {
        model.remove(model.row_count() - 1);
    }
}

pub fn apply(window: &ui::PanelWindow, models: &Models, frame: Frame) {
    let global = window.global::<ui::TimerUi>();
    let view = match &frame.body {
        Body::None => "",
        Body::Stopwatch(_) => "stopwatch",
        Body::CountdownConfig(_) => "countdown-config",
        Body::CountdownRun(_) => "countdown-run",
        Body::PomodoroList(_) => "pomodoro-list",
        Body::PomodoroTimer(_) => "pomodoro-timer",
        Body::Wizard(_) => "wizard",
        Body::Overview(_) => "overview",
    };
    global.set_view(view.into());
    global.set_title(frame.title);
    global.set_subtitle(frame.subtitle);
    global.set_icon_kind(frame.icon_kind.into());
    global.set_notice(frame.notice);
    global.set_forgotten_visible(frame.forgotten.is_some());
    global.set_forgotten_text(frame.forgotten.unwrap_or_default());
    match frame.body {
        Body::None => {}
        Body::Stopwatch(f) => stopwatch(&global, models, f),
        Body::CountdownConfig(f) => countdown_config(&global, models, f),
        Body::CountdownRun(f) => countdown_run(&global, f),
        Body::PomodoroList(cards) => sync_by(&models.pomodoros, cards, same_card),
        Body::PomodoroTimer(f) => pomodoro_timer(&global, models, f),
        Body::Wizard(f) => wizard(&global, models, f),
        Body::Overview(cards) => sync(&models.cards, cards),
    }
}

fn stopwatch(global: &ui::TimerUi<'_>, models: &Models, f: StopwatchFrame) {
    global.set_sw_state(f.state);
    global.set_sw_time(f.time);
    global.set_sw_task(f.task);
    global.set_sw_progress_label(f.progress_label);
    global.set_sw_progress(f.progress);
    sync(&models.subtasks, f.subtasks);
}

fn countdown_config(global: &ui::TimerUi<'_>, models: &Models, f: CountdownConfigFrame) {
    let [h, m, s] = f.fields;
    global.set_cd_h(h);
    global.set_cd_m(m);
    global.set_cd_s(s);
    global.set_cd_can_start(f.can_start);
    global.set_cd_task(f.task);
    sync(&models.presets, f.presets);
}

fn countdown_run(global: &ui::TimerUi<'_>, f: CountdownRunFrame) {
    global.set_cd_run_state(f.state);
    global.set_cd_run_status(f.status);
    global.set_cd_run_time(f.time);
    global.set_cd_run_caption(f.caption);
    global.set_cd_run_progress(f.progress);
    global.set_cd_run_task(f.task);
}

fn pomodoro_timer(global: &ui::TimerUi<'_>, models: &Models, f: PomodoroTimerFrame) {
    let v = f.view;
    global.set_pomo_phase(v.phase.into());
    global.set_pomo_break(v.is_break);
    global.set_pomo_time(v.time.into());
    global.set_pomo_focus_label(v.focus_label.into());
    global.set_pomo_note(v.note.into());
    global.set_pomo_progress(v.progress);
    global.set_pomo_primary_label(v.primary_label.into());
    global.set_pomo_started(v.started);
    global.set_pomo_arc(f.arc);
    global.set_pomo_head_x(f.head.0);
    global.set_pomo_head_y(f.head.1);
    global.set_pomo_task(f.task);
    sync(&models.dots, f.dots);
}

fn wizard(global: &ui::TimerUi<'_>, models: &Models, f: WizardFrame) {
    global.set_wz_step(f.step);
    global.set_wz_title(f.title);
    global.set_wz_help(f.help);
    global.set_wz_unit(f.unit);
    global.set_wz_value(f.value);
    global.set_wz_summary(f.summary);
    global.set_wz_name(f.name);
    global.set_wz_name_placeholder(f.placeholder);
    global.set_wz_auto_start(f.auto_start);
    global.set_wz_task(f.task);
    sync(&models.shortcuts, f.shortcuts);
}

/// Pomodoro cards are equal when their fields and their segment bars are.
fn same_card(a: &ui::PomodoroCard, b: &ui::PomodoroCard) -> bool {
    a.id == b.id
        && a.name == b.name
        && a.summary == b.summary
        && a.task == b.task
        && a.parts.iter().eq(b.parts.iter())
}

#[cfg(test)]
mod tests {
    use slint::ModelRc;

    use super::*;

    fn card(name: &str) -> ui::PomodoroCard {
        let parts = vec![
            ui::BarPart {
                fraction: 0.8,
                focus: true,
            },
            ui::BarPart {
                fraction: 0.2,
                focus: false,
            },
        ];
        ui::PomodoroCard {
            id: 1,
            name: name.into(),
            parts: ModelRc::new(VecModel::from(parts)),
            ..Default::default()
        }
    }

    #[test]
    fn sync_keeps_equal_rows_and_resizes() {
        let model = VecModel::from(vec![1, 2, 3]);
        sync(&model, vec![1, 5]);
        assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5]);
        sync(&model, vec![1, 5, 7, 8]);
        assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5, 7, 8]);
        sync(&model, Vec::new());
        assert_eq!(model.row_count(), 0);
    }

    #[test]
    fn an_unchanged_pomodoro_card_is_left_alone_between_ticks() {
        let model = VecModel::default();
        sync_by(&model, vec![card("Clássico")], same_card);
        let first = model.row_data(0).unwrap().parts;
        // The next tick builds the same card again, with a new segment model.
        sync_by(&model, vec![card("Clássico")], same_card);
        assert!(
            model.row_data(0).unwrap().parts == first,
            "the row was replaced"
        );
        sync_by(&model, vec![card("Deep work")], same_card);
        assert_eq!(model.row_data(0).unwrap().name, "Deep work");
    }
}
