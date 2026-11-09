//! Wires the `TimerUi` callbacks to [`TimerPanels`]. Every closure holds a weak reference, so the
//! window never keeps the panels alive.

use std::rc::{Rc, Weak};

use slint::ComponentHandle;

use super::TimerPanels;
use super::actions_overview::TaskTarget;
use super::convert::timer_from_ui;
use crate::ui;

/// `move || with(&weak, |p| ...)`: runs `f` while the panels are alive.
fn with(weak: &Weak<TimerPanels>, f: impl FnOnce(&TimerPanels)) {
    if let Some(panels) = weak.upgrade() {
        f(&panels);
    }
}

pub fn connect(panels: &Rc<TimerPanels>) {
    let Some(window) = panels.ui.upgrade() else {
        return;
    };
    let global = window.global::<ui::TimerUi>();
    let weak = Rc::downgrade(panels);
    stopwatch(&global, &weak);
    countdown(&global, &weak);
    countdown_run(&global, &weak);
    pomodoro(&global, &weak);
    wizard(&global, &weak);
    overview(&global, &weak);
}

fn stopwatch(g: &ui::TimerUi<'_>, weak: &Weak<TimerPanels>) {
    let w = weak.clone();
    g.on_dismiss_notice(move || with(&w, TimerPanels::dismiss_notice));
    let w = weak.clone();
    g.on_sw_primary(move || with(&w, TimerPanels::stopwatch_primary));
    let w = weak.clone();
    g.on_sw_stop(move || with(&w, TimerPanels::stopwatch_stop));
    let w = weak.clone();
    g.on_sw_change_task(move || with(&w, |p| p.choose_task(TaskTarget::Stopwatch)));
    let w = weak.clone();
    g.on_sw_toggle_subtask(move |id, done| with(&w, |p| p.toggle_subtask(id, done)));
    let w = weak.clone();
    g.on_sw_add_subtask(move |name| with(&w, |p| p.add_subtask(&name)));
    let w = weak.clone();
    g.on_forgotten_record(move || with(&w, |p| p.forgotten_answer(true)));
    let w = weak.clone();
    g.on_forgotten_discard(move || with(&w, |p| p.forgotten_answer(false)));
}

fn countdown(g: &ui::TimerUi<'_>, weak: &Weak<TimerPanels>) {
    let w = weak.clone();
    g.on_cd_step(move |field, up| with(&w, |p| p.countdown_step(field, up)));
    let w = weak.clone();
    g.on_cd_add_minutes(move |m| with(&w, |p| p.countdown_add_minutes(m)));
    let w = weak.clone();
    g.on_cd_start(move || with(&w, TimerPanels::countdown_start));
    let w = weak.clone();
    g.on_cd_change_task(move || with(&w, |p| p.choose_task(TaskTarget::Countdown)));
    let w = weak.clone();
    g.on_cd_start_preset(move |id| with(&w, |p| p.countdown_start_preset(id)));
    let w = weak.clone();
    g.on_cd_load_preset(move |id| with(&w, |p| p.countdown_load_preset(id)));
    let w = weak.clone();
    g.on_cd_save_preset(move |name| with(&w, |p| p.countdown_save_preset(&name)));
}

fn countdown_run(g: &ui::TimerUi<'_>, weak: &Weak<TimerPanels>) {
    let w = weak.clone();
    g.on_cd_run_primary(move || with(&w, TimerPanels::countdown_run_primary));
    let w = weak.clone();
    g.on_cd_run_cancel(move || with(&w, TimerPanels::countdown_cancel));
    let w = weak.clone();
    g.on_cd_run_dismiss(move || with(&w, |p| p.dismiss(None)));
    let w = weak.clone();
    g.on_cd_run_change_task(move || with(&w, |p| p.choose_task(TaskTarget::CountdownRun)));
}

fn pomodoro(g: &ui::TimerUi<'_>, weak: &Weak<TimerPanels>) {
    let w = weak.clone();
    g.on_pomo_open_preset(move |id| with(&w, |p| p.pomodoro_open_preset(id)));
    let w = weak.clone();
    g.on_pomo_new(move || with(&w, TimerPanels::wizard_open));
    let w = weak.clone();
    g.on_pomo_primary(move || with(&w, TimerPanels::pomodoro_primary));
    let w = weak.clone();
    g.on_pomo_stop(move || with(&w, TimerPanels::pomodoro_stop));
    let w = weak.clone();
    g.on_pomo_skip(move || with(&w, TimerPanels::pomodoro_skip));
    let w = weak.clone();
    g.on_pomo_change_task(move || with(&w, |p| p.choose_task(TaskTarget::Pomodoro)));
}

fn wizard(g: &ui::TimerUi<'_>, weak: &Weak<TimerPanels>) {
    let w = weak.clone();
    g.on_wz_bump(move |up| with(&w, |p| p.wizard_edit(|wz| wz.bump(up))));
    let w = weak.clone();
    g.on_wz_set(move |value| {
        let value = u32::try_from(value).unwrap_or(0);
        with(&w, |p| p.wizard_edit(|wz| wz.set_value(value)));
    });
    let w = weak.clone();
    g.on_wz_next(move || with(&w, TimerPanels::wizard_next));
    let w = weak.clone();
    g.on_wz_back(move || with(&w, TimerPanels::wizard_back));
    let w = weak.clone();
    g.on_wz_toggle_auto(move || with(&w, |p| p.wizard_edit(|wz| wz.auto_start = !wz.auto_start)));
    let w = weak.clone();
    g.on_wz_name_edited(move |name| {
        // No render: the text field already shows what was typed.
        with(&w, |p| p.state.borrow_mut().wizard.name = name.to_string());
    });
    let w = weak.clone();
    g.on_wz_change_task(move || with(&w, |p| p.choose_task(TaskTarget::Wizard)));
}

fn overview(g: &ui::TimerUi<'_>, weak: &Weak<TimerPanels>) {
    let w = weak.clone();
    g.on_card_open(move |id| {
        if let Some(id) = timer_from_ui(id) {
            with(&w, |p| p.open_card(id));
        }
    });
    let w = weak.clone();
    g.on_card_primary(move |id| {
        if let Some(id) = timer_from_ui(id) {
            with(&w, |p| p.toggle(id));
        }
    });
    let w = weak.clone();
    g.on_card_stop(move |id| {
        if let Some(id) = timer_from_ui(id) {
            with(&w, |p| {
                p.stop(id);
            });
        }
    });
    let w = weak.clone();
    g.on_card_dismiss(move |id| {
        if let Some(id) = timer_from_ui(id) {
            with(&w, |p| p.dismiss(Some(id)));
        }
    });
    let w = weak.clone();
    g.on_open_menu(move || with(&w, TimerPanels::open_menu));
    let w = weak.clone();
    g.on_new_timer(move || with(&w, TimerPanels::new_timer));
    let w = weak.clone();
    g.on_cycle(move |forward| with(&w, |p| p.cycle(forward)));
}
