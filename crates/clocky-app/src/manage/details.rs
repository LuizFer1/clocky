//! Description and estimated time of the task page (the task's own and its subtasks').
// Slint callbacks hand their arguments over by value.
#![allow(clippy::needless_pass_by_value)]

use std::time::Duration;

use clocky_core::domain::TaskId;
use slint::SharedString;

use super::{Manage, NO_INVALID_ESTIMATE, TASK_ESTIMATE, db_id};
use crate::core::{AppCore, CoreResult};
use crate::format;

impl Manage {
    pub(super) fn connect_details(&self) {
        let state = self.state();
        state.on_set_task_description(self.handler2(Self::set_task_description));
        state.on_set_task_estimate(self.handler1(Self::set_task_estimate));
        state.on_open_link(|url| open_link(&url));
    }

    /// Saves the description typed for `id` (which may no longer be the open task).
    fn set_task_description(&self, id: i32, text: SharedString) {
        let Some(task) = db_id(id).map(TaskId) else {
            return;
        };
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

/// Opens a link of a description in the default browser (or mail app). Other schemes (files,
/// scripts) are ignored: a description must not be able to start a program.
fn open_link(url: &str) {
    if !format::markdown::is_openable(url) {
        tracing::warn!("link not opened (not a web or mail link): {url}");
        return;
    }
    #[cfg(windows)]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    if let Err(e) = command.arg(url).spawn() {
        tracing::warn!("failed to open the link {url}: {e}");
    }
}
