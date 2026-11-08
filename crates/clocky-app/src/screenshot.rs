//! Debug hook for visual checks. With `CLOCKY_SCREENSHOT_DIR` set, the app renders each of its
//! own windows to PNG files and exits. It never captures the desktop: `Window::take_snapshot`
//! renders the window's own scene, without what is behind it.

use std::path::{Path, PathBuf};

/// Environment variable that turns the screenshot mode on.
pub const ENV_VAR: &str = "CLOCKY_SCREENSHOT_DIR";

/// The output directory, when the screenshot mode is on.
pub fn dir_from_env() -> Option<PathBuf> {
    std::env::var_os(ENV_VAR)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// Saves `<dir>/<name>-<suffix>.png` for every window, logging failures.
pub fn save_all(dir: &Path, suffix: &str, windows: &[(&str, &slint::Window)]) {
    for (name, window) in windows {
        let path = dir.join(format!("{name}-{suffix}.png"));
        match save(window, &path) {
            Ok(()) => tracing::info!("screenshot saved: {}", path.display()),
            Err(e) => tracing::warn!("screenshot {} failed: {e:#}", path.display()),
        }
    }
}

fn save(window: &slint::Window, path: &Path) -> anyhow::Result<()> {
    let shot = window.take_snapshot()?;
    image::save_buffer(
        path,
        shot.as_bytes(),
        shot.width(),
        shot.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_window_list_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        save_all(dir.path(), "dark", &[]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
