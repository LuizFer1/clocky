//! Tray icon with "Abrir Gerenciar" and "Sair". Created once the event loop runs (required on
//! macOS). A failure only logs a warning: the dock and the manage window keep working.

use crate::ui;

/// Creates and shows the tray icon. The returned handle must stay alive: dropping it removes
/// the icon.
pub fn create(
    on_open_manage: impl Fn() + 'static,
    on_quit: impl Fn() + 'static,
) -> Option<ui::ClockyTray> {
    let tray = match ui::ClockyTray::new() {
        Ok(tray) => tray,
        Err(e) => {
            tracing::warn!("failed to create the tray icon: {e}");
            return None;
        }
    };
    tray.on_open_manage(on_open_manage);
    tray.on_quit(on_quit);
    if let Err(e) = tray.show() {
        tracing::warn!("failed to show the tray icon: {e}");
        return None;
    }
    Some(tray)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tray_icon_rasterizes_to_rgba() {
        // The Windows tray converts the icon with `to_rgba8`; an SVG must render to pixels.
        let icon = slint::Image::load_from_svg_data(include_bytes!("../icons/tray.svg")).unwrap();
        let pixels = icon.to_rgba8().unwrap();
        assert_eq!((pixels.width(), pixels.height()), (32, 32));
    }
}
