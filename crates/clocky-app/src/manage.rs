//! Manage window (projects and tasks). Closing it only hides it: the app stays in the tray.

use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;

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

    /// Shows the window and brings it to the front. Only for user actions (tray, second launch,
    /// radial menu): on Windows `focus_window` simulates an Alt key press to get past the
    /// foreground lock.
    pub fn bring_to_front(&self) {
        if !self.show() {
            return;
        }
        self.ui.window().with_winit_window(|w| {
            w.set_minimized(false);
            w.focus_window();
        });
    }

    pub fn set_dark(&self, dark: bool) {
        self.ui.global::<ui::Theme>().set_dark(dark);
    }
}
