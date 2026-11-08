//! The Alarme panel's behavior: the `AlarmUi` callbacks change the [`Draft`] and push it back.
//! Text fields are two-way bound, so their `edited` callbacks only update the draft (and the
//! summary): pushing the text back while the user types would move the cursor.

use std::rc::{Rc, Weak};

use clocky_core::domain::AlarmId;
use slint::{ComponentHandle, ModelRc, VecModel};

use super::draft::{Draft, two_digits};
use super::{Alarms, Event, summary};
use crate::ui;

fn with(weak: &Weak<Alarms>, f: impl FnOnce(&Alarms)) {
    if let Some(alarms) = weak.upgrade() {
        f(&alarms);
    }
}

pub fn connect(alarms: &Rc<Alarms>, g: &ui::AlarmUi<'_>) {
    let weak = Rc::downgrade(alarms);
    let w = weak.clone();
    g.on_step_hour(move |up| with(&w, |a| a.edit(|d| d.step_hour(up))));
    let w = weak.clone();
    g.on_step_minute(move |up| with(&w, |a| a.edit(|d| d.step_minute(up))));
    let w = weak.clone();
    g.on_hour_edited(move |t| with(&w, |a| a.typed(|d| d.set_hour_text(&t))));
    let w = weak.clone();
    g.on_minute_edited(move |t| with(&w, |a| a.typed(|d| d.set_minute_text(&t))));
    let w = weak.clone();
    g.on_set_repeating(move |on| with(&w, |a| a.edit(|d| d.repeating = on)));
    let w = weak.clone();
    g.on_toggle_day(move |i| {
        let index = usize::try_from(i).unwrap_or(usize::MAX);
        with(&w, |a| a.edit(|d| d.toggle_day(index)));
    });
    let w = weak.clone();
    g.on_name_edited(move |t| with(&w, |a| a.draft.borrow_mut().name = t.to_string()));
    let w = weak.clone();
    g.on_message_edited(move |t| with(&w, |a| a.draft.borrow_mut().message = t.to_string()));
    let w = weak.clone();
    g.on_sound_edited(move |t| with(&w, |a| a.sound_edited(&t)));
    let w = weak.clone();
    g.on_bump_snooze(move |up| with(&w, |a| a.edit(|d| d.bump_snooze(up))));
    let w = weak.clone();
    g.on_change_task(move || with(&w, Alarms::choose_task));
    let w = weak.clone();
    g.on_preview(move || with(&w, Alarms::preview));
    let w = weak.clone();
    g.on_cancel(move || with(&w, |a| a.emit(Event::Cancelled)));
    let w = weak.clone();
    g.on_save(move || with(&w, Alarms::save));
    let w = weak.clone();
    g.on_delete(move || with(&w, Alarms::delete));
    connect_cards(g, &weak);
}

fn connect_cards(g: &ui::AlarmUi<'_>, weak: &Weak<Alarms>) {
    let w = weak.clone();
    g.on_card_open(move |id| with(&w, |a| a.open_card(AlarmId(i64::from(id)))));
    let w = weak.clone();
    g.on_card_toggle(move |id, on| with(&w, |a| a.toggle(AlarmId(i64::from(id)), on)));
    let w = weak.clone();
    g.on_card_dismiss(move |id| with(&w, |a| a.dismiss(AlarmId(i64::from(id)))));
}

/// The editor.
impl Alarms {
    /// "Alarme" in the radial menu: a new alarm at the next whole hour.
    pub fn start_new(&self) {
        let now = self.core.borrow().now();
        *self.draft.borrow_mut() = Draft::at_next_hour(now, &chrono::Local);
        self.push(true);
    }

    /// Loads a saved alarm into the editor (without changing the view). `false` when it is gone.
    pub fn edit_alarm(&self, id: AlarmId) -> bool {
        let loaded = self.core.borrow().alarm(id);
        match loaded {
            Ok(alarm) => {
                let mut draft = Draft::from_alarm(&alarm);
                // A task deleted meanwhile would fail the save on its foreign key.
                if draft.task.is_some() && self.task_label(draft.task).is_none() {
                    draft.task = None;
                }
                *self.draft.borrow_mut() = draft;
                self.push(true);
                true
            }
            Err(e) => {
                tracing::warn!("cannot open alarm {}: {e}", id.0);
                false
            }
        }
    }

    /// A card click in Ativos.
    fn open_card(&self, id: AlarmId) {
        if self.edit_alarm(id) {
            self.emit(Event::OpenEditor {
                from_overview: true,
            });
        }
    }

    /// Ativos toggle. Switching off a ringing alarm also stops it.
    fn toggle(&self, id: AlarmId, on: bool) {
        if let Err(e) = self.core.borrow_mut().set_alarm_enabled(id, on) {
            tracing::error!(
                "cannot switch alarm {} {}: {e}",
                id.0,
                if on { "on" } else { "off" }
            );
        }
        if !on {
            self.dismiss(id);
        }
        self.emit(Event::Changed);
    }

    /// A change from an arrow, chip or stepper: everything is pushed again.
    fn edit(&self, change: impl FnOnce(&mut Draft)) {
        change(&mut self.draft.borrow_mut());
        self.push(false);
    }

    /// Typed hours or minutes: only the summary follows, the field keeps what was typed.
    fn typed(&self, change: impl FnOnce(&mut Draft) -> bool) {
        if change(&mut self.draft.borrow_mut()) {
            self.push_summary();
        }
    }

    fn sound_edited(&self, text: &str) {
        let path = text.trim();
        {
            let mut draft = self.draft.borrow_mut();
            draft.sound = (!path.is_empty()).then(|| path.to_owned());
        }
        if let Some(ui) = self.panel.upgrade() {
            let label = summary::sound_label(Some(path));
            ui.global::<ui::AlarmUi>().set_sound_label(label.into());
        }
    }

    /// ▶ of the sound: plays it once; a file that fails plays the chime and says so.
    fn preview(&self) {
        let sound = self.draft.borrow().sound.clone();
        let notice = self.ringer.preview(sound.as_deref()).unwrap_or_default();
        self.set_notice(&notice);
    }

    fn choose_task(&self) {
        let current = self.draft.borrow().task;
        let weak = self.weak.clone();
        self.chooser.choose(
            current,
            Box::new(move |task| {
                with(&weak, |a| a.edit(|d| d.task = task));
            }),
        );
    }

    fn save(&self) {
        let draft = self.draft.borrow().clone();
        let Some(fields) = draft.to_new() else {
            return;
        };
        let saved = {
            let mut core = self.core.borrow_mut();
            match draft.editing {
                Some(id) => core.update_alarm(id, &fields),
                None => core.create_alarm(&fields).map(drop),
            }
        };
        match saved {
            Ok(()) => {
                let text = draft.summary();
                let mut lower = text.chars();
                let first = lower.next().map(|c| c.to_lowercase().to_string());
                let notice = format!(
                    "Alarme salvo: {}{}",
                    first.unwrap_or_default(),
                    lower.as_str()
                );
                self.emit(Event::Closed { notice });
            }
            Err(e) => {
                tracing::error!("cannot save the alarm: {e}");
                self.set_notice("Não foi possível salvar o alarme.");
            }
        }
    }

    fn delete(&self) {
        let Some(id) = self.draft.borrow().editing else {
            return;
        };
        if let Err(e) = self.core.borrow_mut().delete_alarm(id) {
            tracing::error!("cannot delete alarm {}: {e}", id.0);
            self.set_notice("Não foi possível apagar o alarme.");
            return;
        }
        self.dismiss(id);
        self.emit(Event::Closed {
            notice: "Alarme apagado".to_owned(),
        });
    }

    fn set_notice(&self, notice: &str) {
        if let Some(ui) = self.panel.upgrade() {
            ui.global::<ui::AlarmUi>().set_notice(notice.into());
        }
    }

    fn push_summary(&self) {
        let Some(ui) = self.panel.upgrade() else {
            return;
        };
        let g = ui.global::<ui::AlarmUi>();
        let draft = self.draft.borrow();
        g.set_summary(draft.summary().into());
        g.set_can_save(draft.can_save());
    }

    /// Pushes the whole draft to `AlarmUi` (the editor shows it). `fresh` (an editor that just
    /// opened) also clears the notice, the delete confirmation and the sound path field.
    fn push(&self, fresh: bool) {
        let Some(ui) = self.panel.upgrade() else {
            return;
        };
        let draft = self.draft.borrow().clone();
        let task = self.task_line(draft.task);
        let g = ui.global::<ui::AlarmUi>();
        g.set_hh(two_digits(draft.hour).into());
        g.set_mm(two_digits(draft.minute).into());
        g.set_summary(draft.summary().into());
        g.set_repeating(draft.repeating);
        let days: Vec<bool> = (0..7).map(|i| draft.day_on(i)).collect();
        g.set_days(ModelRc::new(VecModel::from(days)));
        g.set_name(draft.name.as_str().into());
        g.set_task(task);
        g.set_sound_label(summary::sound_label(draft.sound.as_deref()).into());
        g.set_sound_path(draft.sound.clone().unwrap_or_default().into());
        g.set_snooze(i32::try_from(draft.snooze).unwrap_or(5));
        g.set_message(draft.message.as_str().into());
        g.set_can_save(draft.can_save());
        g.set_editing(draft.editing.is_some());
        if fresh {
            g.set_notice(slint::SharedString::new());
            g.set_confirm_delete(false);
            g.set_sound_editing(false);
        }
    }
}
