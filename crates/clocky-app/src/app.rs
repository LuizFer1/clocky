//! Application state. Everything here lives on the UI thread.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use crate::manage::Manage;
use crate::screenshot;

/// Time for the windows to be placed and rendered before a screenshot.
const SCREENSHOT_DELAY: Duration = Duration::from_millis(1500);

pub struct App {
    pub manage: Manage,
}

impl App {
    pub fn new() -> anyhow::Result<Rc<Self>> {
        Ok(Rc::new(Self {
            manage: Manage::new()?,
        }))
    }

    /// Shows the windows. Runs before the event loop starts.
    pub fn start(self: &Rc<Self>) {
        // Plain `show`: a newly started app gets the foreground from the OS anyway.
        if !self.manage.show() {
            tracing::warn!("the manage window could not be shown");
        }
    }

    /// Switches every window between the dark and the light theme.
    pub fn set_dark(&self, dark: bool) {
        self.manage.set_dark(dark);
    }

    fn windows(&self) -> Vec<(&'static str, &slint::Window)> {
        vec![("manage", self.manage.ui.window())]
    }

    /// Screenshot mode: captures every window in the dark theme, then in the light one, and quits.
    pub fn schedule_screenshots(self: &Rc<Self>, dir: PathBuf) {
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(SCREENSHOT_DELAY, move || {
            let Some(app) = weak.upgrade() else { return };
            screenshot::save_all(&dir, "dark", &app.windows());
            app.set_dark(false);
            let weak = Rc::downgrade(&app);
            slint::Timer::single_shot(SCREENSHOT_DELAY, move || {
                if let Some(app) = weak.upgrade() {
                    screenshot::save_all(&dir, "light", &app.windows());
                }
                quit();
            });
        });
    }
}

/// Ends the event loop (tray "Sair").
pub fn quit() {
    if let Err(e) = slint::quit_event_loop() {
        tracing::error!("failed to quit the event loop: {e}");
    }
}
