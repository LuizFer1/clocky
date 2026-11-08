//! The "alarme tocando" window (`ui/alarm-overlay.slint`): shown without taking the focus (on
//! Windows `focus_window` simulates an Alt press, and nothing the user clicked opened it), always
//! on top, centered near the top of the dock's monitor.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, PhysicalPosition};

use crate::dock::hide_from_taskbar;
use crate::geometry::{Anchor, Point, Rect, to_px};
use crate::ui;

thread_local! {
    /// Set while the overlay's window is created, so the attributes hook recognizes it.
    static CREATING: Cell<bool> = const { Cell::new(false) };
}

/// Window attributes hook of the backend (`main.rs`): the overlay's window is created inactive,
/// so showing it does not take the focus (on Windows the first show uses `SW_SHOWNOACTIVATE`).
/// Every other window keeps the defaults.
pub fn window_attributes(
    attributes: slint::winit_030::winit::window::WindowAttributes,
) -> slint::winit_030::winit::window::WindowAttributes {
    if CREATING.with(Cell::get) {
        attributes.with_active(false)
    } else {
        attributes
    }
}

/// Width of the overlay (logical px, `alarm-overlay.slint`).
pub const OVERLAY_W: f64 = 360.0;
/// Its top edge sits this share of the monitor height below the monitor's top.
const TOP_SHARE: f64 = 0.16;

/// What the overlay shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OverlayView {
    pub name: String,
    pub time: String,
    pub message: String,
    pub task: ui::TaskLine,
    /// "atrasado 12 min"; empty when on time.
    pub late: String,
    /// "1 de 2"; empty with one alarm.
    pub position: String,
    pub snooze_minutes: i32,
    pub notice: String,
}

/// Top-left corner of the overlay on `monitor` (physical px): centered horizontally, near the
/// top.
pub fn origin(monitor: Rect, scale: f64) -> Point {
    let width = to_px(OVERLAY_W, scale);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a monitor is a few thousand px"
    )]
    let top = (f64::from(monitor.h) * TOP_SHARE).round() as i32;
    Point {
        x: monitor.x + (monitor.w - width) / 2,
        y: monitor.y + top,
    }
}

/// "1 de 2" when several alarms ring.
pub fn position_label(index: usize, total: usize) -> String {
    if total > 1 {
        format!("{} de {total}", index + 1)
    } else {
        String::new()
    }
}

pub struct Overlay {
    pub ui: ui::AlarmOverlay,
    prepared: Cell<bool>,
    weak: Weak<Overlay>,
}

impl Overlay {
    pub fn new() -> anyhow::Result<Rc<Self>> {
        // The attributes hook (`window_attributes`) applies to the window created here only.
        CREATING.with(|c| c.set(true));
        let ui = ui::AlarmOverlay::new();
        CREATING.with(|c| c.set(false));
        let ui = ui?;
        Ok(Rc::new_cyclic(|weak| Self {
            ui,
            prepared: Cell::new(false),
            weak: weak.clone(),
        }))
    }

    pub fn set_theme(&self, mode: crate::settings::ThemeMode) {
        mode.apply(&self.ui.global::<ui::Theme>());
    }

    /// Fills and shows the window (placed on the dock's monitor when known).
    pub fn show(&self, view: OverlayView, anchor: Option<Anchor>) {
        let ui = &self.ui;
        ui.set_name(view.name.into());
        ui.set_time(view.time.into());
        ui.set_message(view.message.into());
        ui.set_task(view.task);
        ui.set_late(view.late.into());
        ui.set_position(view.position.into());
        ui.set_snooze_minutes(view.snooze_minutes);
        ui.set_notice(view.notice.into());
        if let Some(anchor) = anchor {
            let at = origin(anchor.monitor, anchor.scale);
            ui.window().set_position(PhysicalPosition::new(at.x, at.y));
        }
        if let Err(e) = ui.show() {
            tracing::warn!("failed to show the alarm overlay: {e}");
            return;
        }
        self.prepare();
    }

    pub fn hide(&self) {
        if let Err(e) = self.ui.hide() {
            tracing::warn!("failed to hide the alarm overlay: {e}");
        }
    }

    /// No taskbar button, once the winit window exists.
    fn prepare(&self) {
        if self.prepared.replace(true) {
            return;
        }
        let weak = self.weak.clone();
        let spawned = slint::spawn_local(async move {
            let Some(overlay) = weak.upgrade() else {
                return;
            };
            match overlay.ui.window().winit_window().await {
                Ok(window) => hide_from_taskbar(&window),
                Err(e) => tracing::warn!("alarm overlay window is not available: {e}"),
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("failed to prepare the alarm overlay: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_is_centered_near_the_top_of_the_monitor() {
        let monitor = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(origin(monitor, 1.0), Point { x: 780, y: 173 });
        let second = Rect {
            x: 1920,
            y: -200,
            w: 2560,
            h: 1440,
        };
        // 360 logical px at 150% = 540 px wide.
        assert_eq!(
            origin(second, 1.5),
            Point {
                x: 1920 + 1010,
                y: -200 + 230
            }
        );
    }

    #[test]
    fn position_shows_only_with_several_alarms() {
        assert_eq!(position_label(0, 1), "");
        assert_eq!(position_label(0, 3), "1 de 3");
    }

    #[test]
    fn only_the_overlay_window_is_created_inactive() {
        use slint::winit_030::winit::window::WindowAttributes;
        assert!(window_attributes(WindowAttributes::default()).active);
        CREATING.with(|c| c.set(true));
        let inactive = window_attributes(WindowAttributes::default());
        CREATING.with(|c| c.set(false));
        assert!(!inactive.active);
    }
}
