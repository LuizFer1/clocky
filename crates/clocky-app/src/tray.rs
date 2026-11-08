//! Tray icon with "Abrir Gerenciar", "Mostrar ícone flutuante" and "Sair". Created once the
//! event loop runs (required on macOS). A failure only logs a warning: the dock and the manage
//! window keep working.

use crate::ui;

/// What the tray menu items do.
pub struct Actions {
    pub open_manage: Box<dyn Fn()>,
    pub show_dock: Box<dyn Fn()>,
    pub quit: Box<dyn Fn()>,
}

/// Creates and shows the tray icon. The returned handle must stay alive: dropping it removes
/// the icon.
pub fn create(actions: Actions) -> Option<ui::ClockyTray> {
    let tray = match ui::ClockyTray::new() {
        Ok(tray) => tray,
        Err(e) => {
            tracing::warn!("failed to create the tray icon: {e}");
            return None;
        }
    };
    let Actions {
        open_manage,
        show_dock,
        quit,
    } = actions;
    tray.on_open_manage(open_manage);
    tray.on_show_dock(show_dock);
    tray.on_quit(quit);
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
