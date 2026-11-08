//! Application state. Everything here lives on the UI thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use clocky_core::clock::SystemClock;
use slint::ComponentHandle;

use crate::bridge::{self, Bridge};
use crate::command::Command;
use crate::core::{AppCore, Reopened};
use crate::dock::Dock;
use crate::manage::Manage;
use crate::panel::{Panel, PanelKind};
use crate::settings::{self, DockPlacement, Settings, ThemeMode};
use crate::{notify, sound};
use crate::{tray, ui};
// M6b: timer panels.
use crate::timers::{self, TimerPanels};
// M7b: Configurações.
use crate::settings_view::Applied;

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
    /// Screenshot mode: no tray, and the settings never come from the user's files.
    screenshot_mode: bool,
    /// Keeps the tray icon alive (dropping it removes the icon).
    tray: RefCell<Option<ui::ClockyTray>>,
    bridge: Bridge<SystemClock>,
    tick_timer: slint::Timer,
    /// M6b: the timer panels and the tracker.
    timers: Rc<TimerPanels>,
    /// M7a: the alarms (Alarme panel, Ativos cards, ringing overlay).
    alarms: Rc<crate::alarms::Alarms>,
    /// 0.2: automatic updates (Configurações > Atualizações).
    updater: Rc<crate::update::Updater>,
}

impl App {
    /// Creates the windows and registers the app for [`run_command`].
    pub fn new(
        core: AppCore,
        reopened: &Reopened,
        screenshot_mode: bool,
    ) -> anyhow::Result<Rc<Self>> {
        // The old `settings.json` is imported once; screenshot runs never read it.
        let legacy = settings::default_path().filter(|_| !screenshot_mode);
        let mut core = core;
        let settings = settings::load(&mut core, legacy.as_deref());
        let core = Rc::new(RefCell::new(core));
        let panel = Panel::new(core.clone())?;
        let alerts: Box<dyn timers::Alerts> = if screenshot_mode {
            Box::new(timers::SilentAlerts)
        } else {
            Box::new(timers::SystemAlerts)
        };
        // M7a: the alarms share the panel window (editor, Ativos cards) and its task picker.
        let ringer: Box<dyn crate::alarms::Ringer> = if screenshot_mode {
            Box::new(crate::alarms::SilentRinger)
        } else {
            Box::new(crate::alarms::SystemRinger::default())
        };
        let alarms = crate::alarms::Alarms::new(
            core.clone(),
            &panel.ui,
            Box::new(timers::PanelChooser::new(Rc::downgrade(&panel))),
            ringer,
        )?;
        let hooks = timers::Hooks {
            chooser: Box::new(timers::PanelChooser::new(Rc::downgrade(&panel))),
            alerts,
            interactive: !screenshot_mode,
            alarms: alarms.clone(),
        };
        let timers = TimerPanels::new(&panel.ui, core.clone(), reopened, hooks);
        let updater = crate::update::Updater::new(core.clone(), !screenshot_mode);
        let app = Rc::new(Self {
            dock: Dock::new()?,
            panel,
            manage: Manage::new(core.clone())?,
            core,
            settings: RefCell::new(settings),
            screenshot_mode,
            tray: RefCell::new(None),
            bridge: Bridge::new(SystemClock),
            tick_timer: slint::Timer::default(),
            timers,
            alarms,
            updater,
        });
        app.connect();
        app.connect_settings();
        app.connect_updates();
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
        // A task created in the picker shows up in Gerenciar right away.
        let weak = Rc::downgrade(self);
        self.panel.picker.set_on_created(move |_| {
            if let Some(app) = weak.upgrade() {
                app.manage.refresh();
            }
        });
        // "Iniciar pomodoro / cronômetro" of a task page: start it and show its panel. Deferred
        // so the manage window's callback has returned before the panel takes the focus.
        let weak = Rc::downgrade(self);
        self.manage.set_on_start_timer(move |kind, task| {
            let weak = weak.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                let Some(app) = weak.upgrade() else { return };
                let panel = app.timers.start_for_task(kind, task);
                app.open_panel(panel, true);
                app.manage.refresh();
            });
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
        // M6b: "Abrir menu" of the empty Ativos list.
        let weak = Rc::downgrade(self);
        self.timers.set_on_open_menu(move || {
            if let Some(app) = weak.upgrade() {
                app.back_to_menu();
            }
        });
        self.connect_alarms();
    }

    /// Applies the theme and shows the dock. Runs before the event loop starts; the tray is
    /// created right after the loop starts (required on macOS).
    pub fn start(self: &Rc<Self>) -> anyhow::Result<()> {
        let settings = self.settings.borrow().clone();
        self.set_theme(settings.theme);
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
        tracing::trace!("tick at {} ms", snapshot.now.as_millis());
        // M7a: alarms first, so Ativos shows a ring in this very tick.
        let missed = self.alarms.tick();
        self.timers.add_notices(&missed);
        // M6b: the tracker drives the badge and may ask for a panel (forgotten stopwatch).
        let outcome = self.timers.tick();
        let badge = outcome
            .active_count
            .saturating_add(self.alarms.ringing_count());
        self.dock.ui.set_active_count(badge);
        if let Some(kind) = outcome.open {
            self.open_panel(kind, false);
        }
    }

    fn start_tray(&self) {
        let tray = tray::create(tray::Actions {
            open_manage: Box::new(|| run_command(Command::Show)),
            show_dock: Box::new(|| run_command(Command::ShowDock)),
            quit: Box::new(|| run_command(Command::Quit)),
        });
        *self.tray.borrow_mut() = tray;
    }

    fn handle(&self, command: Command) {
        tracing::info!("command: {}", command.as_str());
        match command {
            Command::Show => self.manage.bring_to_front(),
            Command::ShowDock => self.dock.recover(),
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
            self.open_panel(kind, true);
        } else {
            tracing::warn!("unknown radial option: {id}");
        }
    }

    /// Shows `kind`'s panel next to the dock with its view set. `focus` for user clicks only
    /// (see [`Panel::open`]).
    pub fn open_panel(&self, kind: PanelKind, focus: bool) {
        self.timers.show(kind);
        self.panel.open(kind, self.dock.anchor(), focus);
    }

    fn back_to_menu(&self) {
        // M6b: nested timer views go back inside the panel first.
        if self.timers.back() {
            return;
        }
        self.panel.hide();
        self.dock.reopen_menu();
    }

    /// Applies the theme mode to every window (Slint globals are per window).
    pub fn set_theme(&self, mode: ThemeMode) {
        self.dock.set_theme(mode);
        self.panel.set_theme(mode);
        self.manage.set_theme(mode);
        self.alarms.set_theme(mode);
    }

    /// The dark or the light theme (screenshot passes).
    pub fn set_dark(&self, dark: bool) {
        self.set_theme(ThemeMode::from_dark(dark));
    }

    fn save_dock(&self, placement: DockPlacement) {
        if let Err(e) = settings::save_dock(&mut self.core.borrow_mut(), &placement) {
            tracing::warn!("failed to save the dock position: {e:#}");
        }
        self.settings.borrow_mut().dock = Some(placement);
    }

    /// Screenshot mode: the tour of `tour.rs` (dark, then light), then quit.
    /// Nothing here takes the focus, so no key press is simulated on the user's desktop.
    pub fn schedule_screenshots(self: &Rc<Self>, dir: PathBuf) {
        // With the demo data (`CLOCKY_DEMO=1`), the timer panel captures follow the tour.
        let next = dir.clone();
        crate::tour::run(self, dir, move |app| {
            if crate::demo::requested() {
                app.schedule_timer_shots(next);
            } else {
                quit();
            }
        });
    }
}

/// M6b: screenshots of the timer panels.
impl App {
    fn schedule_timer_shots(self: &Rc<Self>, dir: PathBuf) {
        let (weak_set, weak_open) = (Rc::downgrade(self), Rc::downgrade(self));
        // M7a, then M7b: the alarm captures follow, then the Relatórios / Configurações tour,
        // then the app quits.
        let (weak_done, dir_done) = (Rc::downgrade(self), dir.clone());
        timers::shots::start(timers::shots::ShotContext {
            panels: self.timers.clone(),
            window: self.panel.ui.as_weak(),
            dir,
            set_dark: Box::new(move |dark| {
                if let Some(app) = weak_set.upgrade() {
                    app.set_dark(dark);
                }
            }),
            open_panel: Box::new(move |kind| {
                if let Some(app) = weak_open.upgrade() {
                    app.open_panel(kind, false);
                }
            }),
            // M7a: the alarm captures follow (then the M7b tour).
            done: Box::new(move || {
                if let Some(app) = weak_done.upgrade() {
                    app.schedule_alarm_shots(dir_done.clone());
                }
            }),
        });
    }
}

/// M7b: Configurações. The theme and the wizard defaults apply right away; the real autostart
/// entry is only used by interactive runs (screenshot runs keep the in-memory one).
impl App {
    fn connect_settings(self: &Rc<Self>) {
        if !self.screenshot_mode
            && let Some(backend) = crate::autostart::system()
        {
            // An entry left by an exe that moved points at this one again; never for a
            // development build (`0.0.0`), which would steal the installed app's entry.
            let release_build = self.updater.current().is_release();
            match std::env::current_exe().map(|exe| {
                if release_build {
                    crate::autostart::repair(backend.as_ref(), &exe)
                } else {
                    Ok(false)
                }
            }) {
                Ok(Ok(true)) => tracing::info!("autostart entry pointed at the running exe"),
                Ok(Ok(false)) => {}
                Ok(Err(e)) | Err(e) => tracing::warn!("cannot check the autostart entry: {e}"),
            }
            self.manage.set_autostart_backend(backend);
        }
        let weak = Rc::downgrade(self);
        self.manage.set_on_settings(move |applied| {
            let Some(app) = weak.upgrade() else { return };
            match applied {
                Applied::Theme(mode) => {
                    app.settings.borrow_mut().theme = mode;
                    app.set_theme(mode);
                }
                Applied::Wizard => app.timers.reload_wizard_defaults(),
                Applied::Presets => app.timers.render(),
                Applied::Stored => {}
            }
        });
    }
}

/// M7a: alarms.
impl App {
    fn connect_alarms(self: &Rc<Self>) {
        // The overlay opens on the dock's monitor.
        let weak = Rc::downgrade(self);
        self.alarms
            .set_anchor_source(move || weak.upgrade().and_then(|app| app.dock.anchor()));
        // An alarm of the task page's "Vinculado a" opens its editor. Deferred so the manage
        // window's callback has returned before the panel takes the focus.
        let weak = Rc::downgrade(self);
        self.manage
            .ui
            .global::<ui::ManageState>()
            .on_open_alarm(move |id| {
                let weak = weak.clone();
                slint::Timer::single_shot(Duration::ZERO, move || {
                    if let Some(app) = weak.upgrade() {
                        app.open_alarm(clocky_core::domain::AlarmId(i64::from(id)));
                    }
                });
            });
    }

    /// Shows the Alarme panel on a saved alarm.
    pub fn open_alarm(&self, id: clocky_core::domain::AlarmId) {
        if self.timers.show_alarm(id) {
            self.panel.open(PanelKind::Alarm, self.dock.anchor(), true);
        }
    }

    fn schedule_alarm_shots(self: &Rc<Self>, dir: PathBuf) {
        // M7b: the Relatórios / Configurações tour runs after the alarm captures.
        let (weak, manage_dir) = (Rc::downgrade(self), dir.clone());
        let then = Box::new(move || crate::tour_manage::run(&weak, manage_dir.clone()));
        crate::alarms::shots::start(self, self.alarms.clone(), self.timers.clone(), dir, then);
    }
}

/// Runs a command on the UI thread (tray menu, second launch).
pub fn run_command(command: Command) {
    let app = APP.with(|cell| cell.borrow().clone());
    if let Some(app) = app {
        app.handle(command);
    } else if !crate::startup::run_command(command) {
        tracing::warn!("command {command:?} arrived before the app was ready");
    }
}

/// Releases the app after the event loop ends. The thread-local keeps an `Rc<App>`, which would
/// otherwise live until the process dies and skip the tray's `Drop` (the icon would stay in the
/// tray until hovered).
pub fn shutdown(app: Rc<App>) {
    let registered = APP.with(|cell| cell.borrow_mut().take());
    drop(registered);
    drop(app);
}

/// Ends the event loop (tray "Sair").
pub fn quit() {
    if let Err(e) = slint::quit_event_loop() {
        tracing::error!("failed to quit the event loop: {e}");
    }
}

/// 0.2: Configurações > Atualizações.
impl App {
    fn connect_updates(self: &Rc<Self>) {
        let s = self.manage.ui.global::<ui::SettingsState>();
        s.set_version(self.updater.current().to_string().into());
        s.set_auto_update(self.updater.auto());
        let weak = Rc::downgrade(self);
        s.on_update_act(move || {
            if let Some(app) = weak.upgrade() {
                app.updater.act();
            }
        });
        let weak = Rc::downgrade(self);
        s.on_set_auto_update(move |on| {
            if let Some(app) = weak.upgrade() {
                app.updater.set_auto(on);
                app.manage
                    .ui
                    .global::<ui::SettingsState>()
                    .set_auto_update(on);
            }
        });
        let weak = Rc::downgrade(self);
        self.updater.set_on_view(move |view| {
            let Some(app) = weak.upgrade() else { return };
            let s = app.manage.ui.global::<ui::SettingsState>();
            s.set_update_status(view.status.as_str().into());
            s.set_update_action(view.action.as_str().into());
            s.set_update_busy(view.busy);
        });
        let weak = Rc::downgrade(self);
        self.updater
            .set_idle_check(move || weak.upgrade().is_some_and(|app| app.is_idle()));
        self.updater.start();
    }

    /// A good moment to restart for an update: no window open and nothing running or ringing
    /// (the dock badge counts both).
    fn is_idle(&self) -> bool {
        !self.panel.ui.window().is_visible()
            && !self.manage.ui.window().is_visible()
            && self.dock.ui.get_active_count() == 0
            && !self.dock.ui.get_open()
    }
}
