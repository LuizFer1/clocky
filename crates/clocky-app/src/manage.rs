//! Manage window (projects and tasks). Closing it only hides it: the app stays in the tray.

use slint::ComponentHandle;

use crate::ui;

pub struct Manage {
    pub ui: ui::ManageWindow,
}

impl Manage {
    pub fn new() -> anyhow::Result<Self> {
        let ui = ui::ManageWindow::new()?;
        ui.window()
            .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
        ui.on_new_project(|| tracing::info!("new project requested (the form arrives in M6)"));
        Ok(Self { ui })
    }

    /// Shows the window without taking the focus. Returns `false` when it cannot be shown.
    pub fn show(&self) -> bool {
        if let Err(e) = self.ui.show() {
            tracing::warn!("failed to show the manage window: {e}");
            return false;
        }
        true
    }

    pub fn set_dark(&self, dark: bool) {
        self.ui.global::<ui::Theme>().set_dark(dark);
    }
}
