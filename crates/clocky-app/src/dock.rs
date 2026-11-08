//! Floating dock window: placement at the monitor edge, plan B collapse/expand (the window
//! shrinks to the hub so clicks reach the apps behind it), drag and snap between monitors, and
//! closing a ring the cursor never entered.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use slint::winit_030::{WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition};

use crate::geometry::{self, Anchor, Monitor, Point, Rect};
use crate::settings::DockPlacement;
use crate::ui;

/// Ring close animation (260ms) plus some slack before the window shrinks.
const COLLAPSE_DELAY: Duration = Duration::from_millis(300);
/// After the panel's back button reopens the ring, it closes unless the cursor enters the menu
/// within this time. `has-hover` never changes if the cursor is elsewhere (M0 bug).
const REOPEN_GRACE: Duration = Duration::from_millis(1500);
/// How often the dock checks that it is still on screen, where its anchor puts it.
const WATCHDOG: Duration = Duration::from_secs(2);
/// Checks in a row with the window off its anchor before it is moved back (a resize between
/// the collapsed and the open size can be in flight during one check).
const DRIFT_CHECKS: u8 = 2;

type PlacedCallback = Box<dyn Fn(DockPlacement)>;

pub struct Dock {
    pub ui: ui::DockWindow,
    anchor: Cell<Option<Anchor>>,
    collapse_timer: slint::Timer,
    reopen_timer: slint::Timer,
    dragging: Cell<bool>,
    /// Where the dock was put, to put it back when the monitors change.
    placement: RefCell<Option<DockPlacement>>,
    /// The monitors at the last check.
    monitors_seen: RefCell<Vec<Monitor>>,
    /// Checks in a row that found the window away from its anchor.
    drift: Cell<u8>,
    watchdog: slint::Timer,
    on_placed: RefCell<Option<PlacedCallback>>,
    weak: Weak<Dock>,
}

impl Dock {
    pub fn new() -> anyhow::Result<Rc<Self>> {
        let ui = ui::DockWindow::new()?;
        // The window is created open (320×360): on Windows a window with a caption style created
        // 30px wide is widened to ~136px. It collapses once it exists.
        ui.set_compact(false);
        let dock = Rc::new_cyclic(|weak| Self {
            ui,
            anchor: Cell::new(None),
            collapse_timer: slint::Timer::default(),
            reopen_timer: slint::Timer::default(),
            dragging: Cell::new(false),
            placement: RefCell::new(None),
            monitors_seen: RefCell::new(Vec::new()),
            drift: Cell::new(0),
            watchdog: slint::Timer::default(),
            on_placed: RefCell::new(None),
            weak: weak.clone(),
        });
        dock.connect_callbacks();
        Ok(dock)
    }

    fn connect_callbacks(&self) {
        let w = self.weak.clone();
        self.ui.on_open_changed(move |open| {
            if let Some(d) = w.upgrade() {
                d.on_open_changed(open);
            }
        });
        let w = self.weak.clone();
        self.ui.on_hub_drag(move |dx, dy| {
            if let Some(d) = w.upgrade() {
                d.on_drag(f64::from(dx), f64::from(dy));
            }
        });
        let w = self.weak.clone();
        self.ui.on_hub_drag_end(move || {
            if let Some(d) = w.upgrade() {
                d.on_drag_end();
            }
        });
    }

    /// Called with the new placement after every drag, to persist it.
    pub fn set_on_placed(&self, callback: impl Fn(DockPlacement) + 'static) {
        *self.on_placed.borrow_mut() = Some(Box::new(callback));
    }

    pub fn anchor(&self) -> Option<Anchor> {
        self.anchor.get()
    }

    pub fn set_theme(&self, mode: crate::settings::ThemeMode) {
        mode.apply(&self.ui.global::<ui::Theme>());
    }

    /// Shows the dock at the saved placement (or the right edge of the current monitor).
    ///
    /// The winit window only exists once the event loop runs, so placement waits for it.
    pub fn show_at(&self, saved: Option<DockPlacement>) -> anyhow::Result<()> {
        self.ui.show()?;
        let weak = self.weak.clone();
        slint::spawn_local(async move {
            let Some(dock) = weak.upgrade() else { return };
            match dock.ui.window().winit_window().await {
                Ok(window) => dock.place_initially(&window, saved.as_ref()),
                Err(e) => tracing::error!("dock window is not available: {e}"),
            }
        })?;
        Ok(())
    }

    fn place_initially(&self, window: &winit::window::Window, saved: Option<&DockPlacement>) {
        // Without this, Windows enforces the minimum width of a window with a caption (~136px).
        window.set_min_inner_size(Some(winit::dpi::PhysicalSize::new(1_u32, 1_u32)));
        hide_from_taskbar(window);
        self.placement.borrow_mut().clone_from(&saved.cloned());
        self.place(window);
        // Collapse through the "cursor left" path: collapsing on the very first frame makes
        // Windows widen the window (caption style); at run time it never does.
        self.on_open_changed(false);
        let weak = self.weak.clone();
        self.watchdog
            .start(slint::TimerMode::Repeated, WATCHDOG, move || {
                if let Some(d) = weak.upgrade() {
                    d.check();
                }
            });
    }

    /// Anchors the dock at its placement (or the right edge of the primary monitor).
    fn place(&self, window: &winit::window::Window) {
        let monitors = monitors(window);
        let primary = window.primary_monitor().map(|m| to_monitor(&m).rect);
        let current = window.current_monitor().map(|m| to_monitor(&m).rect);
        let fallback = geometry::fallback_monitor(&monitors, primary, current);
        let placement = self.placement.borrow().clone();
        let saved = placement
            .as_ref()
            .map(|p| (p.monitor.as_deref().unwrap_or(""), p.edge, p.y_fraction));
        if let Some((anchor, _)) = geometry::restore(&monitors, saved, fallback) {
            self.set_anchor(anchor);
        } else {
            tracing::warn!("no monitor found; the dock stays where the OS put it");
        }
        *self.monitors_seen.borrow_mut() = monitors;
        self.drift.set(0);
    }

    /// Watchdog: brings the dock back when the OS minimized or hid it, when the monitors
    /// changed (a monitor unplugged or asleep, a resolution or scale change) and when the
    /// window ended up away from its anchor. Every repair is logged.
    fn check(&self) {
        if self.dragging.get() {
            return;
        }
        let Some(repair) = self.ui.window().with_winit_window(|w| self.diagnose(w)) else {
            return;
        };
        match repair {
            Repair::Nothing => {}
            Repair::Show => {
                tracing::warn!("the dock window was minimized or hidden; showing it again");
                self.restore_window();
            }
            Repair::Replace => {
                tracing::info!("the monitors changed; placing the dock again");
                self.ui.window().with_winit_window(|w| self.place(w));
            }
            Repair::Relayout(actual) => {
                tracing::warn!("the dock window was at {actual:?}; moving it back to its anchor");
                self.drift.set(0);
                self.apply_layout();
            }
        }
    }

    fn diagnose(&self, window: &winit::window::Window) -> Repair {
        if window.is_minimized() == Some(true) || window.is_visible() == Some(false) {
            return Repair::Show;
        }
        let monitors = monitors(window);
        if monitors.is_empty() {
            // Every monitor asleep or switching: wait for them to come back.
            return Repair::Nothing;
        }
        if *self.monitors_seen.borrow() != monitors {
            return Repair::Replace;
        }
        let Some(anchor) = self.anchor.get() else {
            return Repair::Nothing;
        };
        let Ok(pos) = window.outer_position() else {
            return Repair::Nothing;
        };
        let size = window.inner_size();
        let actual = Rect {
            x: pos.x,
            y: pos.y,
            w: i32::try_from(size.width).unwrap_or(i32::MAX),
            h: i32::try_from(size.height).unwrap_or(i32::MAX),
        };
        let expected = anchor.window_rect(self.ui.get_compact());
        let off_screen = !geometry::on_some_monitor(&monitors, actual);
        if !off_screen && !geometry::rect_drifted(expected, actual) {
            self.drift.set(0);
            return Repair::Nothing;
        }
        // A resize between the collapsed and the open size may still be on its way.
        self.drift.set(self.drift.get().saturating_add(1));
        if off_screen || self.drift.get() >= DRIFT_CHECKS {
            Repair::Relayout(actual)
        } else {
            Repair::Nothing
        }
    }

    /// Shows the dock again where it belongs (tray "Mostrar ícone flutuante" and the watchdog).
    pub fn recover(&self) {
        self.restore_window();
        self.reopen_menu();
    }

    /// Un-minimizes and shows the window, on top, at its anchor.
    fn restore_window(&self) {
        if let Err(e) = self.ui.show() {
            tracing::warn!("failed to show the dock: {e}");
        }
        self.ui.window().with_winit_window(|w| {
            w.set_minimized(false);
            // winit skips `set_visible(true)` when it believes the window is already visible
            // (hidden behind its back), so it is hidden first.
            w.set_visible(false);
            w.set_visible(true);
            // Same for the level: going through `Normal` makes winit apply it again.
            w.set_window_level(winit::window::WindowLevel::Normal);
            w.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
            self.place(w);
        });
    }

    fn set_anchor(&self, anchor: Anchor) {
        self.anchor.set(Some(anchor));
        self.apply_layout();
    }

    /// Moves and resizes the window for the current anchor and state (collapsed or open).
    fn apply_layout(&self) {
        let Some(anchor) = self.anchor.get() else {
            return;
        };
        self.ui.set_left_side(anchor.is_left());
        let rect = anchor.window_rect(self.ui.get_compact());
        self.ui
            .window()
            .set_position(PhysicalPosition::new(rect.x, rect.y));
        // Slint resizes from the `compact` property too, but the size request must go through
        // winit with a 1×1 minimum or Windows keeps the caption-style minimum width.
        let size = winit::dpi::PhysicalSize::new(
            u32::try_from(rect.w).unwrap_or(1),
            u32::try_from(rect.h).unwrap_or(1),
        );
        self.ui.window().with_winit_window(|w| {
            w.set_min_inner_size(Some(winit::dpi::PhysicalSize::new(1_u32, 1_u32)));
            // `None` means the resize is applied asynchronously, which is fine here.
            if w.request_inner_size(size).is_some() {
                tracing::debug!("dock resized synchronously");
            }
        });
    }

    fn on_open_changed(&self, open: bool) {
        if open {
            self.collapse_timer.stop();
            if self.ui.get_compact() {
                self.ui.set_compact(false);
                self.apply_layout();
            }
            return;
        }
        self.reopen_timer.stop();
        let weak = self.weak.clone();
        self.collapse_timer
            .start(slint::TimerMode::SingleShot, COLLAPSE_DELAY, move || {
                if let Some(d) = weak.upgrade()
                    && !d.ui.get_open()
                    && !d.dragging.get()
                {
                    d.ui.set_compact(true);
                    d.apply_layout();
                }
            });
    }

    /// Follows the cursor while the hub is dragged (offset since the press, logical px).
    fn on_drag(&self, dx: f64, dy: f64) {
        self.dragging.set(true);
        let window = self.ui.window();
        let pos = window.position();
        let scale = f64::from(window.scale_factor());
        // The pressed point stays fixed in the window, so adding the offset is enough.
        window.set_position(PhysicalPosition::new(
            pos.x + geometry::to_px(dx, scale),
            pos.y + geometry::to_px(dy, scale),
        ));
    }

    /// On release, sticks to the nearest side edge of the monitor under the hub and saves it.
    fn on_drag_end(&self) {
        self.dragging.set(false);
        let window = self.ui.window();
        let pos = window.position();
        let edge = self.anchor.get().map(|a| a.edge).unwrap_or_default();
        let hub = geometry::open_hub_center(
            Point { x: pos.x, y: pos.y },
            f64::from(window.scale_factor()),
            edge,
        );
        let monitors = window.with_winit_window(monitors).unwrap_or_default();
        let Some((anchor, monitor)) = geometry::snap(&monitors, hub) else {
            return;
        };
        let placement = DockPlacement {
            monitor: monitor.name.clone(),
            edge: anchor.edge,
            y_fraction: anchor.y_fraction(),
        };
        tracing::info!("dock dropped at {hub:?}, snapped to {placement:?}");
        *self.placement.borrow_mut() = Some(placement.clone());
        self.set_anchor(anchor);
        if let Some(callback) = self.on_placed.borrow().as_ref() {
            callback(placement);
        }
    }

    /// Reopens the ring (panel's back button and the manage window's "Menu" link).
    pub fn reopen_menu(&self) {
        self.ui.set_compact(false);
        self.ui.set_open(true);
        self.apply_layout();
        let weak = self.weak.clone();
        self.reopen_timer
            .start(slint::TimerMode::SingleShot, REOPEN_GRACE, move || {
                if let Some(d) = weak.upgrade()
                    && !d.ui.get_pointer_inside()
                {
                    d.ui.set_open(false);
                }
            });
    }

    /// Closes the ring (when a panel opens).
    pub fn close_menu(&self) {
        self.ui.set_open(false);
    }
}

/// What the watchdog found.
#[derive(Debug)]
enum Repair {
    Nothing,
    /// Minimized or hidden by the OS.
    Show,
    /// The monitors changed: anchor again from the placement.
    Replace,
    /// The window is away from its anchor (or off every monitor), at this rectangle.
    Relayout(Rect),
}

/// The dock and the panel get no taskbar button (Slint has no property for it). winit only
/// offers this on Windows for an existing window; Linux and macOS are manual checks for now.
pub fn hide_from_taskbar(window: &winit::window::Window) {
    #[cfg(windows)]
    {
        use slint::winit_030::winit::platform::windows::WindowExtWindows;
        window.set_skip_taskbar(true);
    }
    #[cfg(not(windows))]
    let _ = window;
}

fn monitors(window: &winit::window::Window) -> Vec<Monitor> {
    window
        .available_monitors()
        .map(|m| to_monitor(&m))
        .collect()
}

fn to_monitor(m: &winit::monitor::MonitorHandle) -> Monitor {
    let (pos, size) = (m.position(), m.size());
    Monitor {
        name: m.name(),
        rect: Rect {
            x: pos.x,
            y: pos.y,
            w: i32::try_from(size.width).unwrap_or(i32::MAX),
            h: i32::try_from(size.height).unwrap_or(i32::MAX),
        },
        scale: m.scale_factor(),
    }
}
