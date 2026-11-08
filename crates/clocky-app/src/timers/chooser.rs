//! Choosing the task of a timer.
//!
//! [`TaskChooser`] is the narrow seam the panels depend on; [`PanelChooser`] opens the task
//! picker of the panel window ([`Panel::pick_task`]: search, recents, groups, create).

use std::rc::Weak;

use clocky_core::domain::TaskId;

use crate::panel::Panel;

/// Called with the chosen task (`None` = "Sem tarefa"). Not called when the user cancels.
pub type OnChosen = Box<dyn FnOnce(Option<TaskId>)>;

pub trait TaskChooser {
    /// Opens the chooser with `current` marked as selected.
    fn choose(&self, current: Option<TaskId>, on_chosen: OnChosen);
    /// Closes the chooser if it is open, without calling its `on_chosen`.
    fn cancel(&self);
}

/// The task picker inside the panel window.
pub struct PanelChooser {
    panel: Weak<Panel>,
}

impl PanelChooser {
    pub fn new(panel: Weak<Panel>) -> Self {
        Self { panel }
    }
}

impl TaskChooser for PanelChooser {
    fn choose(&self, current: Option<TaskId>, on_chosen: OnChosen) {
        if let Some(panel) = self.panel.upgrade() {
            panel.pick_task(current, on_chosen);
        }
    }

    fn cancel(&self) {
        if let Some(panel) = self.panel.upgrade()
            && panel.picker.is_open()
        {
            panel.picker.finish(false);
        }
    }
}
