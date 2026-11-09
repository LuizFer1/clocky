//! Description and estimated time of the task page (the task's own and its subtasks').
// Slint callbacks hand their arguments over by value.
#![allow(clippy::needless_pass_by_value)]

use std::time::Duration;

use slint::SharedString;

use super::{Manage, NO_INVALID_ESTIMATE, TASK_ESTIMATE};
use crate::core::{AppCore, CoreResult};
use crate::format;

impl Manage {
    pub(super) fn connect_details(&self) {
        let state = self.state();
        state.on_set_task_description(self.handler1(Self::set_task_description));
        state.on_set_task_estimate(self.handler1(Self::set_task_estimate));
    }

    fn set_task_description(&self, text: SharedString) {
        let Some(task) = self.nav().task else { return };
        self.mutate("save the task description", |c| {
            c.set_task_description(task, text.trim())
        });
    }

    fn set_task_estimate(&self, text: SharedString) {
        let Some(task) = self.nav().task else { return };
        self.save_estimate(TASK_ESTIMATE, &text, |c, estimate| {
            c.set_task_estimate(task, estimate)
        });
    }

    /// Saves a typed estimate through `save`. Text that is not an estimate saves nothing and
    /// shows the hint under the field `field` (`-1` = the task, else the subtask id).
    pub(super) fn save_estimate(
        &self,
        field: i32,
        text: &str,
        save: impl FnOnce(&mut AppCore, Option<Duration>) -> CoreResult<()>,
    ) {
        let Ok(estimate) = format::parse_estimate(text) else {
            self.state().set_estimate_invalid(field);
            return;
        };
        self.state().set_estimate_invalid(NO_INVALID_ESTIMATE);
        self.mutate("save the estimate", |c| save(c, estimate));
    }
}
