//! Application state. Everything here lives on the UI thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use crate::dock::Dock;
use crate::manage::Manage;
use crate::panel::{Panel, PanelKind};
use crate::screenshot;
use crate::settings::{self, DockPlacement, Settings};

/// Time for the windows to be placed and rendered before a screenshot.
const SCREENSHOT_DELAY: Duration = Duration::from_millis(1500);

pub struct App {
    pub dock: Rc<Dock>,
    pub panel: Rc<Panel>,
    pub manage: Manage,
    settings: RefCell<Settings>,
    /// `None` when settings must not be written (no config dir, or screenshot mode).
    settings_path: Option<PathBuf>,
}

impl App {
    /// Creates the windows. `persist = false` leaves the settings file untouched.
    pub fn new(persist: bool) -> anyhow::Result<Rc<Self>> {
        let path = settings::default_path();
        let settings = path.as_deref().map(Settings::load).unwrap_or_default();
        let app = Rc::new(Self {
            dock: Dock::new()?,
            panel: Panel::new()?,
            manage: Manage::new()?,
            settings: RefCell::new(settings),
            settings_path: path.filter(|_| persist),
        });
        app.connect();
        Ok(app)
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.dock.set_on_placed(move |placement| {
            if let Some(app) = weak.upgrade() {
                app.save_dock(placement);
            }
        });
        let weak = Rc::downgrade(self);
        self.dock.ui.on_option_selected(move |id| {
            if let Some(app) = weak.upgrade() {
                app.option_selected(&id);
            }
        });
        let weak = Rc::downgrade(self);
        self.manage.ui.on_open_menu(move || {
            if let Some(app) = weak.upgrade() {
                app.dock.reopen_menu();
            }
        });
        // The panel's back button and its "Abrir menu" action both return to the ring.
        let weak = Rc::downgrade(self);
        self.panel.ui.on_back(move || {
            if let Some(app) = weak.upgrade() {
                app.back_to_menu();
            }
        });
        let weak = Rc::downgrade(self);
        self.panel.ui.on_action(move || {
            if let Some(app) = weak.upgrade() {
                app.back_to_menu();
            }
        });
    }

    /// Applies the theme and shows the dock. Runs before the event loop starts.
    pub fn start(self: &Rc<Self>) -> anyhow::Result<()> {
        let settings = self.settings.borrow().clone();
        self.set_dark(settings.dark_theme);
        self.dock.show_at(settings.dock)
    }

    /// A radial option was clicked: "Gerenciar" opens the manage window (spec, section 2), the
    /// others open their panel next to the dock.
    fn option_selected(&self, id: &str) {
        self.dock.close_menu();
        if id == "gerenciar" {
            self.manage.bring_to_front();
            return;
        }
        if let Some(kind) = PanelKind::from_id(id) {
            self.panel.open(kind, self.dock.anchor(), true);
        } else {
            tracing::warn!("unknown radial option: {id}");
        }
    }

    fn back_to_menu(&self) {
        self.panel.hide();
        self.dock.reopen_menu();
    }

    /// Switches every window between the dark and the light theme (Slint globals are per window).
    pub fn set_dark(&self, dark: bool) {
        self.dock.set_dark(dark);
        self.panel.set_dark(dark);
        self.manage.set_dark(dark);
    }

    fn save_dock(&self, placement: DockPlacement) {
        let mut settings = self.settings.borrow_mut();
        settings.dock = Some(placement);
        let Some(path) = &self.settings_path else {
            return;
        };
        if let Err(e) = settings.save(path) {
            tracing::warn!("failed to save {}: {e:#}", path.display());
        }
    }

    fn windows(&self) -> Vec<(&'static str, &slint::Window)> {
        vec![
            ("dock", self.dock.ui.window()),
            ("panel", self.panel.ui.window()),
            ("manage", self.manage.ui.window()),
        ]
    }

    /// Screenshot mode: opens the windows, captures them in the dark theme, then in the light
    /// one, and quits.
    /// Nothing here takes the focus, so no key press is simulated on the user's desktop.
    pub fn schedule_screenshots(self: &Rc<Self>, dir: PathBuf) {
        if !self.manage.show() {
            tracing::warn!("the manage window could not be shown");
        }
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(SCREENSHOT_DELAY / 2, move || {
            if let Some(app) = weak.upgrade() {
                app.panel
                    .open(PanelKind::Overview, app.dock.anchor(), false);
                app.dock.ui.set_open(true);
            }
        });
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(SCREENSHOT_DELAY * 2, move || {
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
