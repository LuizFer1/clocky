//! Application state. Everything here lives on the UI thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use clocky_core::clock::SystemClock;
use slint::ComponentHandle;

use crate::bridge::{self, Bridge};
use crate::command::Command;
use crate::core::AppCore;
use crate::dock::Dock;
use crate::manage::Manage;
use crate::panel::{Panel, PanelKind};
use crate::settings::{self, DockPlacement, Settings};
use crate::{notify, sound};
use crate::{tray, ui};

thread_local! {
    /// The running app, for commands that arrive from other threads (single instance).
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

pub struct App {
    pub dock: Rc<Dock>,
    pub panel: Rc<Panel>,
    pub manage: Rc<Manage>,
    /// The core, shared with the windows that read and change data.
    pub core: Rc<RefCell<AppCore>>,
    settings: RefCell<Settings>,
    /// `None` when settings must not be written (no config dir, or screenshot mode).
    settings_path: Option<PathBuf>,
    /// Screenshot mode: no tray and no settings writes.
    screenshot_mode: bool,
    /// Keeps the tray icon alive (dropping it removes the icon).
    tray: RefCell<Option<ui::ClockyTray>>,
    bridge: Bridge<SystemClock>,
    tick_timer: slint::Timer,
}

impl App {
    /// Creates the windows and registers the app for [`run_command`].
    pub fn new(core: AppCore, screenshot_mode: bool) -> anyhow::Result<Rc<Self>> {
        let path = settings::default_path();
        let settings = path.as_deref().map(Settings::load).unwrap_or_default();
        let core = Rc::new(RefCell::new(core));
        let app = Rc::new(Self {
            dock: Dock::new()?,
            panel: Panel::new(core.clone())?,
            manage: Manage::new(core.clone())?,
            core,
            settings: RefCell::new(settings),
            settings_path: path.filter(|_| !screenshot_mode),
            screenshot_mode,
            tray: RefCell::new(None),
            bridge: Bridge::new(SystemClock),
            tick_timer: slint::Timer::default(),
        });
        app.connect();
        APP.with(|cell| *cell.borrow_mut() = Some(app.clone()));
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
        // Timer panels arrive in M6b: until then the task page buttons are only logged.
        self.manage.set_on_start_timer(|kind, task| {
            tracing::info!(
                "start {kind:?} for task {} requested (timer panels: M6b)",
                task.0
            );
        });
        let weak = Rc::downgrade(self);
        self.manage
            .ui
            .global::<ui::ManageState>()
            .on_open_menu(move || {
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

    /// Applies the theme and shows the dock. Runs before the event loop starts; the tray is
    /// created right after the loop starts (required on macOS).
    pub fn start(self: &Rc<Self>) -> anyhow::Result<()> {
        let settings = self.settings.borrow().clone();
        self.set_dark(settings.dark_theme);
        self.dock.show_at(settings.dock)?;
        let weak = Rc::downgrade(self);
        self.tick_timer
            .start(slint::TimerMode::Repeated, bridge::TICK, move || {
                if let Some(app) = weak.upgrade() {
                    app.on_tick();
                }
            });
        if !self.screenshot_mode {
            let weak = Rc::downgrade(self);
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(app) = weak.upgrade() {
                    app.start_tray();
                }
            });
        }
        Ok(())
    }

    /// 1 Hz: asks the bridge for the current state and pushes it to the UI.
    fn on_tick(&self) {
        let snapshot = self.bridge.tick();
        self.dock.ui.set_active_count(snapshot.active_count);
        tracing::trace!("tick at {} ms", snapshot.now.as_millis());
    }

    fn start_tray(&self) {
        let tray = tray::create(|| run_command(Command::Show), || run_command(Command::Quit));
        *self.tray.borrow_mut() = tray;
    }

    fn handle(&self, command: Command) {
        tracing::info!("command: {}", command.as_str());
        match command {
            Command::Show => self.manage.bring_to_front(),
            Command::Quit => quit(),
            Command::NotifyTest => notify::show("Clocky", "Notificação de teste do Clocky."),
            Command::SoundTest => sound::play_default(),
        }
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

    /// Screenshot mode: the tour of `tour.rs` (dark, then light), then quit.
    /// Nothing here takes the focus, so no key press is simulated on the user's desktop.
    pub fn schedule_screenshots(self: &Rc<Self>, dir: PathBuf) {
        crate::tour::run(self, dir);
    }
}

/// Runs a command on the UI thread (tray menu, second launch).
pub fn run_command(command: Command) {
    let app = APP.with(|cell| cell.borrow().clone());
    if let Some(app) = app {
        app.handle(command);
    } else {
        tracing::warn!("command {command:?} arrived before the app was ready");
    }
}

/// Ends the event loop (tray "Sair").
pub fn quit() {
    if let Err(e) = slint::quit_event_loop() {
        tracing::error!("failed to quit the event loop: {e}");
    }
}
