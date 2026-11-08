//! Alarms in the app (M7a): the Alarme panel (create and edit), the alarm cards of Ativos, and
//! the runtime of the 1 Hz tick (ring with the overlay, sound and notification, or a "perdido"
//! notice), on top of the core's alarm rules (`clocky_core::alarms`, `Store::pending_alarms`).
//!
//! [`Alarms`] lives on the UI thread next to the timer panels, which own the panel's view: it
//! reports navigation through [`Event`]s and the timer panels switch views and re-render.

pub mod cards;
pub mod demo;
mod draft;
mod editor;
mod overlay;
pub use overlay::window_attributes;
pub mod queue;
mod ringer;
mod runtime;
pub mod shots;
pub mod summary;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use clocky_core::domain::{AlarmId, TaskId};
use clocky_core::time::Timestamp;
use slint::ComponentHandle;

pub use cards::AlarmCard;
pub use queue::Ringing;
pub use ringer::{Ringer, SilentRinger, SystemRinger, effective_sound};

use crate::core::AppCore;
use crate::geometry::Anchor;
use crate::timers::TaskChooser;
use crate::{palette, ui};
use draft::Draft;
pub use draft::rearms_on_save;
use overlay::{Overlay, OverlayView};
use queue::Queue;
use runtime::Handled;

/// What the timer panels must do after an alarm action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Show the editor (a card was clicked in Ativos: back returns there).
    OpenEditor { from_overview: bool },
    /// Saved or deleted: go to Ativos with this notice.
    Closed { notice: String },
    /// Cancelar: back to where the editor came from.
    Cancelled,
    /// Something shown in Ativos changed (toggle, ring, dismiss, snooze).
    Changed,
}

type EventHook = Rc<dyn Fn(Event)>;
type AnchorSource = Box<dyn Fn() -> Option<Anchor>>;

pub struct Alarms {
    core: Rc<RefCell<AppCore>>,
    panel: slint::Weak<ui::PanelWindow>,
    chooser: Box<dyn TaskChooser>,
    ringer: Box<dyn Ringer>,
    overlay: Rc<Overlay>,
    draft: RefCell<Draft>,
    queue: RefCell<Queue>,
    handled: RefCell<Handled>,
    /// The ring whose sound plays (alarm and occurrence), and its fallback notice.
    sounding: RefCell<Option<(AlarmId, Timestamp, String)>>,
    /// The check is failing (logged once per streak).
    failing: Cell<bool>,
    on_event: RefCell<Option<EventHook>>,
    anchor: RefCell<Option<AnchorSource>>,
    weak: Weak<Self>,
}

impl Alarms {
    pub fn new(
        core: Rc<RefCell<AppCore>>,
        panel: &ui::PanelWindow,
        chooser: Box<dyn TaskChooser>,
        ringer: Box<dyn Ringer>,
    ) -> anyhow::Result<Rc<Self>> {
        let overlay = Overlay::new()?;
        let alarms = Rc::new_cyclic(|weak| Self {
            core,
            panel: panel.as_weak(),
            chooser,
            ringer,
            overlay,
            draft: RefCell::new(Draft::new(7, 30)),
            queue: RefCell::new(Queue::default()),
            handled: RefCell::new(Handled::new()),
            sounding: RefCell::new(None),
            failing: Cell::new(false),
            on_event: RefCell::new(None),
            anchor: RefCell::new(None),
            weak: weak.clone(),
        });
        editor::connect(&alarms, &panel.global::<ui::AlarmUi>());
        alarms.connect_overlay();
        Ok(alarms)
    }

    /// Where navigation and changes are reported (the timer panels).
    pub fn set_on_event(&self, f: impl Fn(Event) + 'static) {
        *self.on_event.borrow_mut() = Some(Rc::new(f));
    }

    /// Where the dock is, to place the overlay on its monitor.
    pub fn set_anchor_source(&self, f: impl Fn() -> Option<Anchor> + 'static) {
        *self.anchor.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, event: Event) {
        let hook = self.on_event.borrow().clone();
        if let Some(hook) = hook {
            hook(event);
        }
    }

    pub fn set_theme(&self, mode: crate::settings::ThemeMode) {
        self.overlay.set_theme(mode);
    }

    /// The overlay window (screenshots).
    pub fn overlay_window(&self) -> &slint::Window {
        self.overlay.ui.window()
    }

    /// Alarms ringing now (the dock badge counts them with the timers).
    pub fn ringing_count(&self) -> i32 {
        i32::try_from(self.queue.borrow().count()).unwrap_or(i32::MAX)
    }

    /// The panel title while the editor shows.
    pub fn title(&self) -> &'static str {
        self.draft.borrow().title()
    }

    /// The card of every alarm (Ativos). A read failure logs and shows none.
    pub fn cards(&self) -> Vec<AlarmCard> {
        let core = self.core.borrow();
        let alarms = core.alarms().unwrap_or_else(|e| {
            tracing::warn!("cannot read the alarms: {e}");
            Vec::new()
        });
        cards::cards(&alarms, &self.queue.borrow(), core.now(), &chrono::Local)
    }

    /// 1 Hz: rings what came due and returns the notices of missed alarms.
    pub fn tick(&self) -> Vec<String> {
        let checked = {
            let mut core = self.core.borrow_mut();
            let now = core.now();
            runtime::check(
                &mut core,
                now,
                &chrono::Local,
                &mut self.handled.borrow_mut(),
            )
        };
        let (rings, notices) = runtime::outcome(checked, &self.failing);
        if !rings.is_empty() {
            self.present(rings);
        }
        notices
    }

    /// Starts ringing: a notification per alarm, then the overlay and the sound of the first.
    pub fn present(&self, rings: Vec<Ringing>) {
        for ring in rings {
            let task = self
                .task_label(ring.task)
                .map(|(p, t, _)| format!("{p} › {t}"));
            let (title, body) = ringer::notification(&ring, task.as_deref());
            self.ringer.notify(&title, &body);
            self.queue.borrow_mut().push(ring);
        }
        // A new ring is always audible, even when it queues behind one whose sound auto-stopped.
        self.show_queue(true);
        self.emit(Event::Changed);
    }

    /// Dispensar (overlay or Ativos card).
    pub fn dismiss(&self, id: AlarmId) {
        if self.queue.borrow_mut().remove(id).is_some() {
            tracing::info!("alarm {} dismissed", id.0);
            self.refresh_overlay();
            self.emit(Event::Changed);
        }
    }

    /// Soneca of the ring the overlay shows.
    fn snooze_current(&self) {
        let Some(id) = self.queue.borrow().current().map(|r| r.id) else {
            return;
        };
        match self.core.borrow_mut().snooze_alarm(id) {
            Ok(Some(until)) => tracing::info!("alarm {} snoozed until {until:?}", id.0),
            Ok(None) => tracing::info!("alarm {} was disabled: no snooze", id.0),
            Err(e) => tracing::error!("cannot snooze alarm {}: {e}", id.0),
        }
        self.dismiss(id);
    }

    /// Shows the first ring (restarting the sound when it changed), or hides the overlay.
    fn refresh_overlay(&self) {
        self.show_queue(false);
    }

    /// [`Self::refresh_overlay`]; `presented` (new rings just arrived) restarts the sound too.
    pub(super) fn show_queue(&self, presented: bool) {
        let queue = self.queue.borrow();
        let Some(current) = queue.current().cloned() else {
            drop(queue);
            self.sounding.borrow_mut().take();
            self.ringer.stop();
            self.overlay.hide();
            return;
        };
        let total = queue.count();
        drop(queue);
        let sounding = self
            .sounding
            .borrow()
            .as_ref()
            .map(|(id, due, _)| (*id, *due));
        if restart_sound(sounding, (current.id, current.due), presented) {
            let notice = self
                .ringer
                .start(self.sound_for(current.sound.as_deref()).as_deref())
                .unwrap_or_default();
            *self.sounding.borrow_mut() = Some((current.id, current.due, notice));
        }
        let notice = self
            .sounding
            .borrow()
            .as_ref()
            .map(|(_, _, n)| n.clone())
            .unwrap_or_default();
        let view = OverlayView {
            name: summary::display_name(&current.name).to_owned(),
            time: current.time.to_string(),
            message: current.message.trim().to_owned(),
            task: self.task_line(current.task),
            late: current.late.map(summary::late_label).unwrap_or_default(),
            position: overlay::position_label(0, total),
            snooze_minutes: i32::try_from(current.snooze_minutes).unwrap_or(5),
            notice,
        };
        let anchor = self.anchor.borrow().as_ref().and_then(|f| f());
        self.overlay.show(view, anchor);
    }

    fn connect_overlay(&self) {
        let weak = self.weak.clone();
        self.overlay.ui.on_snooze(move || {
            if let Some(a) = weak.upgrade() {
                a.snooze_current();
            }
        });
        let weak = self.weak.clone();
        self.overlay.ui.on_dismiss(move || {
            if let Some(a) = weak.upgrade() {
                a.dismiss_current();
            }
        });
        // Closing the window through the OS (Alt+F4) answers like Dispensar, so no alarm keeps
        // sounding behind a hidden overlay; the next queued one shows, or it hides.
        let weak = self.weak.clone();
        self.overlay.ui.window().on_close_requested(move || {
            if let Some(a) = weak.upgrade() {
                a.dismiss_current();
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
    }

    fn dismiss_current(&self) {
        let current = self.queue.borrow().current().map(|r| r.id);
        if let Some(id) = current {
            self.dismiss(id);
        } else {
            self.overlay.hide();
        }
    }

    /// Project name, task name and color of a task (`None` = no task, or it is gone).
    fn task_label(&self, task: Option<TaskId>) -> Option<(String, String, String)> {
        let core = self.core.borrow();
        let task = core.task(task?).ok()?;
        let (project, color) = match task.project.map(|p| core.project(p)) {
            Some(Ok(p)) => (p.name, p.color),
            _ => (crate::manage_view::NO_PROJECT.to_owned(), String::new()),
        };
        Some((project, task.name, color))
    }

    /// The `● projeto › tarefa` line of a task.
    fn task_line(&self, task: Option<TaskId>) -> ui::TaskLine {
        self.task_label(task)
            .map(|(project, task, color)| ui::TaskLine {
                linked: true,
                project: project.into(),
                task: task.into(),
                color: palette::color(&color),
            })
            .unwrap_or_default()
    }
}

/// M7: the default alarm sound of Configurações.
impl Alarms {
    /// The `alarm.default_sound` setting (`None` when unset, empty or unreadable).
    fn default_sound(&self) -> Option<String> {
        let read = self
            .core
            .borrow()
            .setting(crate::settings::keys::ALARM_SOUND);
        read.unwrap_or_else(|e| {
            tracing::warn!("cannot read the default alarm sound: {e}");
            None
        })
    }

    /// The sound to play for an alarm whose own file is `own` (see [`effective_sound`]).
    fn sound_for(&self, own: Option<&str>) -> Option<String> {
        effective_sound(own, self.default_sound().as_deref())
    }
}

/// Whether the sound must (re)start for the ring shown first: it changed, or new rings were
/// just presented (the loop of an unanswered ring may have auto-stopped meanwhile).
fn restart_sound(
    sounding: Option<(AlarmId, Timestamp)>,
    current: (AlarmId, Timestamp),
    presented: bool,
) -> bool {
    presented || sounding != Some(current)
}

#[cfg(test)]
mod tests {
    use clocky_core::alarms::AlarmTime;

    use super::*;

    fn ring(id: i64, due: i64) -> Ringing {
        Ringing {
            id: AlarmId(id),
            due: Timestamp::from_millis(due),
            name: format!("a{id}"),
            message: String::new(),
            time: AlarmTime::new(7, 30).unwrap(),
            task: None,
            sound: None,
            snooze_minutes: 5,
            late: None,
        }
    }

    #[test]
    fn a_ring_queued_behind_a_silent_one_restarts_the_sound() {
        let a = (AlarmId(1), Timestamp::from_millis(0));
        // Alarm A still shows (its loop auto-stopped); alarm B queues behind it.
        let mut queue = Queue::default();
        queue.push(ring(1, 0));
        assert!(!queue.push(ring(2, 0)));
        let current = queue.current().map(|r| (r.id, r.due)).unwrap();
        assert!(restart_sound(Some(a), current, true));
        // A ring replaced in place behind the first one is a new presentation too.
        assert!(!queue.push(ring(2, 5)));
        assert!(restart_sound(Some(a), current, true));
        // Without new rings, the same ring keeps its sound; a different first ring restarts it.
        assert!(!restart_sound(Some(a), current, false));
        assert!(restart_sound(
            Some(a),
            (AlarmId(2), Timestamp::from_millis(0)),
            false
        ));
        assert!(restart_sound(None, current, false));
    }
}
