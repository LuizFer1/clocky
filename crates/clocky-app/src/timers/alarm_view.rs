//! M7a: how the panel follows the alarms. [`crate::alarms::Alarms`] owns the editor and the
//! ringing alarms; the timer panels own the panel's view, so alarm actions come back here as
//! [`Event`]s that switch the view, set the notice and re-render.

use std::rc::Rc;

use super::TimerPanels;
use super::state::View;
use crate::alarms::Event;

pub fn connect(panels: &Rc<TimerPanels>) {
    let weak = Rc::downgrade(panels);
    panels.alarms.set_on_event(move |event| {
        if let Some(panels) = weak.upgrade() {
            panels.alarm_event(event);
        }
    });
}

impl TimerPanels {
    fn alarm_event(&self, event: Event) {
        match event {
            Event::OpenEditor { from_overview } => {
                self.chooser.cancel();
                self.set_view(View::Alarm { from_overview });
            }
            Event::Closed { notice } => {
                self.state.borrow_mut().notice = notice;
                self.set_view(View::Overview);
            }
            Event::Cancelled => {
                if !self.back() {
                    self.open_menu();
                }
            }
            Event::Changed => {
                if self.is_visible() {
                    self.render();
                }
            }
        }
    }

    /// Adds lines to the panel's notice (missed alarms, "Alarme perdido às 07:30 · nome"). It
    /// stays until closed or until a panel is opened from the menu after it was seen.
    pub fn add_notices(&self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        {
            let mut state = self.state.borrow_mut();
            for line in lines {
                if !state.notice.is_empty() {
                    state.notice.push('\n');
                }
                state.notice.push_str(line);
            }
            // Not seen yet, even if older lines were.
            state.notice_seen = false;
        }
        self.render();
    }
}
