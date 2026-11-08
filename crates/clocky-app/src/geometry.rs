//! Dock and panel geometry. Pure functions over physical pixels, without winit types, so the
//! snapping, collapse and mirroring rules are unit-tested.

use serde::{Deserialize, Serialize};

/// Open radial menu area, in logical px (handoff SPEC 1).
pub const AREA_W: f64 = 320.0;
pub const AREA_H: f64 = 360.0;
/// Collapsed dock (plan B of the M0 report): half the hub (28) + 2 for the badge, by the hub
/// height (56) + 2 + 2. Everything outside this rectangle lets clicks through.
pub const COMPACT_W: f64 = 30.0;
pub const COMPACT_H: f64 = 60.0;
/// Distance from the screen edge to the open hub's center.
pub const HUB_OPEN_OFFSET: f64 = 105.0;
/// Floating panel (handoff SPEC 2): 380 wide, 30 from the edge, 28 below the area top.
pub const PANEL_W: f64 = 380.0;
pub const PANEL_H: f64 = 420.0;
pub const PANEL_EDGE_GAP: f64 = 30.0;
pub const PANEL_TOP_GAP: f64 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Squared distance from `p` to the rectangle (0 when inside).
    fn distance_sq(&self, p: Point) -> i64 {
        let dx = i64::from((self.x - p.x).max(p.x - self.right()).max(0));
        let dy = i64::from((self.y - p.y).max(p.y - self.bottom()).max(0));
        dx * dx + dy * dy
    }
}

/// Screen edge the dock sticks to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    #[default]
    Right,
}

/// A monitor, in physical px.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub name: Option<String>,
    pub rect: Rect,
    pub scale: f64,
}

/// Rounds logical px times the scale factor to physical px.
#[allow(clippy::cast_possible_truncation)]
pub fn to_px(logical: f64, scale: f64) -> i32 {
    (logical * scale).round() as i32
}

/// Clamps `value` to `[min, max]`, or returns the midpoint when the range is empty
/// (a monitor smaller than the area). `i32::clamp` would panic there.
fn clamp_or_center(value: i32, min: i32, max: i32) -> i32 {
    if min > max {
        min + (max - min) / 2
    } else {
        value.clamp(min, max)
    }
}

/// Where the dock sits: monitor, edge and the vertical center of the hub (physical px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub monitor: Rect,
    pub scale: f64,
    pub edge: Edge,
    pub center_y: i32,
}

impl Anchor {
    /// Builds an anchor, keeping the whole open area inside the monitor's height.
    pub fn new(monitor: &Monitor, edge: Edge, center_y: i32) -> Self {
        let rect = monitor.rect;
        let half = to_px(AREA_H / 2.0, monitor.scale);
        Self {
            monitor: rect,
            scale: monitor.scale,
            edge,
            center_y: clamp_or_center(center_y, rect.y + half, rect.bottom() - half),
        }
    }

    /// The menu is mirrored (pill grows to the right) on the left edge.
    pub fn is_left(&self) -> bool {
        self.edge == Edge::Left
    }

    fn px(&self, logical: f64) -> i32 {
        to_px(logical, self.scale)
    }

    /// Dock window rectangle: the 30×60 hub slice when collapsed, the 320×360 area when open.
    pub fn window_rect(&self, compact: bool) -> Rect {
        let (w, h) = if compact {
            (self.px(COMPACT_W), self.px(COMPACT_H))
        } else {
            (self.px(AREA_W), self.px(AREA_H))
        };
        let x = if self.is_left() {
            self.monitor.x
        } else {
            self.monitor.right() - w
        };
        Rect {
            x,
            y: self.center_y - h / 2,
            w,
            h,
        }
    }

    /// Top-left corner of the floating panel, kept inside the monitor.
    pub fn panel_origin(&self) -> Point {
        let x = if self.is_left() {
            self.monitor.x + self.px(PANEL_EDGE_GAP)
        } else {
            self.monitor.right() - self.px(PANEL_EDGE_GAP + PANEL_W)
        };
        let area_top = self.center_y - self.px(AREA_H / 2.0);
        let y = clamp_or_center(
            area_top + self.px(PANEL_TOP_GAP),
            self.monitor.y,
            self.monitor.bottom() - self.px(PANEL_H),
        );
        Point { x, y }
    }

    /// Hub height as a fraction of the monitor height (DPI and resolution independent).
    pub fn y_fraction(&self) -> f64 {
        if self.monitor.h <= 0 {
            return 0.5;
        }
        f64::from(self.center_y - self.monitor.y) / f64::from(self.monitor.h)
    }
}

/// Center of the open hub, given the top-left corner of the open dock window.
pub fn open_hub_center(window: Point, scale: f64, edge: Edge) -> Point {
    let dx = match edge {
        Edge::Left => HUB_OPEN_OFFSET,
        Edge::Right => AREA_W - HUB_OPEN_OFFSET,
    };
    Point {
        x: window.x + to_px(dx, scale),
        y: window.y + to_px(AREA_H / 2.0, scale),
    }
}

/// The monitor that contains `p`, or the closest one. Ties go to the first in the list.
pub fn nearest_monitor(monitors: &[Monitor], p: Point) -> Option<&Monitor> {
    monitors.iter().min_by_key(|m| m.rect.distance_sq(p))
}

/// Nearest side edge for a hub dropped at `x`. A drop in the exact middle goes right.
pub fn nearest_edge(monitor: Rect, x: i32) -> Edge {
    if x - monitor.x < monitor.right() - x {
        Edge::Left
    } else {
        Edge::Right
    }
}

/// Where a hub dropped at `hub` sticks: nearest monitor, nearest side edge, same height.
pub fn snap(monitors: &[Monitor], hub: Point) -> Option<(Anchor, &Monitor)> {
    let monitor = nearest_monitor(monitors, hub)?;
    let edge = nearest_edge(monitor.rect, hub.x);
    Some((Anchor::new(monitor, edge, hub.y), monitor))
}

/// The monitor to use when the saved one is gone: the primary monitor, else the one the window
/// was created on. Both are matched by their rectangle against `monitors`.
pub fn fallback_monitor(
    monitors: &[Monitor],
    primary: Option<Rect>,
    current: Option<Rect>,
) -> Option<&Monitor> {
    let find = |rect: Rect| monitors.iter().find(|m| m.rect == rect);
    primary.and_then(find).or_else(|| current.and_then(find))
}

/// Restores a saved position: the monitor with that name, else `fallback` (the primary
/// monitor), else the first one. Without a saved position: right edge, vertically centered.
pub fn restore<'a>(
    monitors: &'a [Monitor],
    saved: Option<(&str, Edge, f64)>,
    fallback: Option<&'a Monitor>,
) -> Option<(Anchor, &'a Monitor)> {
    let by_name =
        saved.and_then(|(name, ..)| monitors.iter().find(|m| m.name.as_deref() == Some(name)));
    let monitor = by_name.or(fallback).or_else(|| monitors.first())?;
    let (edge, fraction) = saved.map_or((Edge::Right, 0.5), |(_, edge, f)| (edge, f));
    let center = monitor.rect.y + to_px(fraction.clamp(0.0, 1.0), f64::from(monitor.rect.h));
    Some((Anchor::new(monitor, edge, center), monitor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Main monitor of the M0 test machine: 1920×1080 at the origin.
    fn main_monitor() -> Monitor {
        Monitor {
            name: Some("DISPLAY2".into()),
            rect: Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
            scale: 1.0,
        }
    }

    /// Secondary monitor of the M0 test machine: left of the main one and lower.
    fn left_monitor() -> Monitor {
        Monitor {
            name: Some("DISPLAY1".into()),
            rect: Rect {
                x: -1366,
                y: 310,
                w: 1366,
                h: 768,
            },
            scale: 1.0,
        }
    }

    fn both() -> Vec<Monitor> {
        vec![main_monitor(), left_monitor()]
    }

    #[test]
    fn drop_left_of_center_sticks_left() {
        let (anchor, _) = snap(&both(), Point { x: 600, y: 700 }).unwrap();
        assert_eq!(anchor.edge, Edge::Left);
        assert_eq!(anchor.center_y, 700);
    }

    #[test]
    fn drop_right_of_center_sticks_right() {
        let (anchor, _) = snap(&both(), Point { x: 1300, y: 300 }).unwrap();
        assert_eq!(anchor.edge, Edge::Right);
    }

    #[test]
    fn drop_in_the_exact_middle_sticks_right() {
        let (anchor, _) = snap(&both(), Point { x: 960, y: 540 }).unwrap();
        assert_eq!(anchor.edge, Edge::Right);
    }

    #[test]
    fn odd_width_middle_pixel_sticks_right() {
        let m = Rect {
            x: 0,
            y: 0,
            w: 1365,
            h: 768,
        };
        // 682 is one px closer to the left edge, 683 one px closer to the right one.
        assert_eq!(nearest_edge(m, 682), Edge::Left);
        assert_eq!(nearest_edge(m, 683), Edge::Right);
    }

    #[test]
    fn drop_on_secondary_monitor_with_negative_coordinates() {
        let monitors = both();

        let (anchor, monitor) = snap(&monitors, Point { x: -1100, y: 500 }).unwrap();
        assert_eq!(monitor.name.as_deref(), Some("DISPLAY1"));
        assert_eq!(anchor.edge, Edge::Left);
        assert_eq!(anchor.window_rect(false).x, -1366);
    }

    #[test]
    fn drop_near_the_seam_sticks_to_the_secondary_right_edge() {
        let monitors = both();

        let (anchor, monitor) = snap(&monitors, Point { x: -10, y: 600 }).unwrap();
        assert_eq!(monitor.name.as_deref(), Some("DISPLAY1"));
        assert_eq!(anchor.edge, Edge::Right);
        assert_eq!(anchor.window_rect(true).right(), 0);
    }

    #[test]
    fn middle_of_secondary_monitor_sticks_right() {
        let (anchor, _) = snap(&both(), Point { x: -683, y: 600 }).unwrap();
        assert_eq!(anchor.edge, Edge::Right);
    }

    #[test]
    fn drop_outside_every_monitor_uses_the_closest() {
        // Below the secondary monitor, which ends at y = 1078.
        let monitors = both();

        let (_, monitor) = snap(&monitors, Point { x: -500, y: 1500 }).unwrap();
        assert_eq!(monitor.name.as_deref(), Some("DISPLAY1"));
        // Far right of the main monitor.
        let monitors = both();

        let (anchor, monitor) = snap(&monitors, Point { x: 2500, y: 100 }).unwrap();
        assert_eq!(monitor.name.as_deref(), Some("DISPLAY2"));
        assert_eq!(anchor.edge, Edge::Right);
    }

    #[test]
    fn no_monitor_no_snap() {
        assert!(snap(&[], Point { x: 0, y: 0 }).is_none());
    }

    #[test]
    fn height_is_clamped_so_the_open_area_fits() {
        let top = Anchor::new(&main_monitor(), Edge::Right, 10);
        assert_eq!(top.center_y, 180);
        let bottom = Anchor::new(&main_monitor(), Edge::Right, 5000);
        assert_eq!(bottom.center_y, 1080 - 180);
        let secondary = Anchor::new(&left_monitor(), Edge::Left, 0);
        assert_eq!(secondary.center_y, 310 + 180);
    }

    #[test]
    fn tiny_monitor_centers_instead_of_panicking() {
        let tiny = Monitor {
            name: None,
            rect: Rect {
                x: 0,
                y: 0,
                w: 300,
                h: 200,
            },
            scale: 1.0,
        };
        assert_eq!(Anchor::new(&tiny, Edge::Right, 0).center_y, 100);
    }

    #[test]
    fn collapsed_window_hugs_the_right_edge() {
        let anchor = Anchor::new(&main_monitor(), Edge::Right, 540);
        let r = anchor.window_rect(true);
        assert_eq!(
            r,
            Rect {
                x: 1890,
                y: 510,
                w: 30,
                h: 60
            }
        );
    }

    #[test]
    fn open_window_hugs_the_right_edge() {
        let anchor = Anchor::new(&main_monitor(), Edge::Right, 540);
        let r = anchor.window_rect(false);
        assert_eq!(
            r,
            Rect {
                x: 1600,
                y: 360,
                w: 320,
                h: 360
            }
        );
    }

    #[test]
    fn mirrored_window_starts_at_the_left_edge() {
        let anchor = Anchor::new(&main_monitor(), Edge::Left, 540);
        assert!(anchor.is_left());
        assert_eq!(anchor.window_rect(true).x, 0);
        assert_eq!(anchor.window_rect(false).x, 0);
    }

    #[test]
    fn geometry_scales_to_physical_pixels() {
        let hidpi = Monitor {
            scale: 1.5,
            ..main_monitor()
        };
        let anchor = Anchor::new(&hidpi, Edge::Right, 540);
        let r = anchor.window_rect(false);
        assert_eq!((r.w, r.h), (480, 540));
        assert_eq!(r.right(), 1920);
        assert_eq!(anchor.window_rect(true).w, 45);
    }

    #[test]
    fn panel_sits_30px_from_the_edge_and_28px_below_the_area_top() {
        let right = Anchor::new(&main_monitor(), Edge::Right, 540);
        assert_eq!(right.panel_origin(), Point { x: 1510, y: 388 });
        let left = Anchor::new(&main_monitor(), Edge::Left, 540);
        assert_eq!(left.panel_origin(), Point { x: 30, y: 388 });
    }

    #[test]
    fn panel_stays_inside_the_monitor_at_the_bottom() {
        let low = Anchor::new(&main_monitor(), Edge::Right, 5000);
        assert_eq!(low.panel_origin().y, 1080 - 420);
    }

    #[test]
    fn open_hub_center_matches_the_edge() {
        let right = open_hub_center(Point { x: 1600, y: 360 }, 1.0, Edge::Right);
        assert_eq!(right, Point { x: 1815, y: 540 });
        let left = open_hub_center(Point { x: 0, y: 360 }, 1.0, Edge::Left);
        assert_eq!(left, Point { x: 105, y: 540 });
    }

    #[test]
    fn y_fraction_roundtrips_through_restore() {
        let anchor = Anchor::new(&left_monitor(), Edge::Left, 700);
        let monitors = both();
        let saved = Some(("DISPLAY1", Edge::Left, anchor.y_fraction()));
        let (restored, monitor) = restore(&monitors, saved, monitors.first()).unwrap();
        assert_eq!(monitor.name.as_deref(), Some("DISPLAY1"));
        assert_eq!(restored, anchor);
    }

    #[test]
    fn restore_falls_back_to_primary_when_the_monitor_is_gone() {
        // The saved monitor is unplugged; the primary is not the first one in the list and the
        // window was created on the other monitor.
        let other = Monitor {
            name: Some("DISPLAY3".into()),
            ..left_monitor()
        };
        let monitors = vec![other.clone(), main_monitor()];
        let fallback = fallback_monitor(&monitors, Some(main_monitor().rect), Some(other.rect));
        let saved = Some(("DISPLAY1", Edge::Left, 0.99));
        let (anchor, monitor) = restore(&monitors, saved, fallback).unwrap();
        assert_eq!(monitor.name.as_deref(), Some("DISPLAY2"));
        assert_eq!(anchor.edge, Edge::Left);
        assert_eq!(anchor.center_y, 1080 - 180);
    }

    #[test]
    fn fallback_prefers_the_primary_monitor() {
        let monitors = both();
        let picked = fallback_monitor(
            &monitors,
            Some(main_monitor().rect),
            Some(left_monitor().rect),
        );
        assert_eq!(picked.unwrap().name.as_deref(), Some("DISPLAY2"));
    }

    #[test]
    fn fallback_uses_the_current_monitor_without_a_primary() {
        // Wayland/X11 setups may report no primary monitor.
        let monitors = both();
        let picked = fallback_monitor(&monitors, None, Some(left_monitor().rect));
        assert_eq!(picked.unwrap().name.as_deref(), Some("DISPLAY1"));
    }

    #[test]
    fn fallback_ignores_monitors_that_are_not_in_the_list() {
        let monitors = vec![main_monitor()];
        let gone = left_monitor().rect;
        assert_eq!(fallback_monitor(&monitors, Some(gone), None), None);
        let picked = fallback_monitor(&monitors, Some(gone), Some(main_monitor().rect));
        assert_eq!(picked.unwrap().name.as_deref(), Some("DISPLAY2"));
    }

    #[test]
    fn restore_without_saved_position_is_right_edge_centered() {
        let monitors = both();
        let (anchor, _) = restore(&monitors, None, monitors.get(1)).unwrap();
        assert_eq!(anchor.edge, Edge::Right);
        assert_eq!(anchor.monitor, left_monitor().rect);
        assert_eq!(anchor.center_y, 310 + 384);
    }

    #[test]
    fn restore_clamps_a_height_fraction_outside_0_to_1() {
        // A hand-edited or corrupt settings file.
        let monitors = both();
        let above = Some(("DISPLAY2", Edge::Right, -0.5));
        let (anchor, _) = restore(&monitors, above, None).unwrap();
        assert_eq!(anchor.center_y, 180);
        let below = Some(("DISPLAY2", Edge::Right, 7.0));
        let (anchor, _) = restore(&monitors, below, None).unwrap();
        assert_eq!(anchor.center_y, 1080 - 180);
        let nan = Some(("DISPLAY2", Edge::Right, f64::NAN));
        let (anchor, _) = restore(&monitors, nan, None).unwrap();
        assert!(
            (180..=900).contains(&anchor.center_y),
            "{}",
            anchor.center_y
        );
    }

    #[test]
    fn restore_without_monitors_is_none() {
        assert!(restore(&[], None, None).is_none());
    }
}
