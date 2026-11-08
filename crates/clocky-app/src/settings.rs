//! App settings in the core `settings` table (spec, section 3): the theme and the dock
//! position. Before M7 they lived in `settings.json` in the user config dir; [`load`] imports
//! that file once and never writes, renames or deletes it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::AppCore;
use crate::geometry::Edge;

/// Keys of the `settings` table written by the app. The core owns `forgotten_limit_ms` and the
/// timer panels read the `pomodoro_*` wizard defaults.
pub mod keys {
    /// `dark` (the default), `light` or `system`.
    pub const THEME: &str = "theme";
    /// The dock position as JSON ([`super::DockPlacement`]).
    pub const DOCK: &str = "dock_placement";
    /// Default alarm sound: a file path, or empty for the built-in sound (read by the alarms).
    pub const ALARM_SOUND: &str = "alarm.default_sound";
    /// Set once the old `settings.json` import was tried (whatever came of it), so it never runs
    /// again: a file that becomes readable later must not overwrite newer settings.
    pub const LEGACY_IMPORTED: &str = "settings_json_imported";
}

const LIGHT: &str = "light";
const DARK: &str = "dark";
const SYSTEM: &str = "system";

/// The theme picked in Configurações.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThemeMode {
    /// The default (spec, decision 10).
    #[default]
    Dark,
    Light,
    /// Follows the OS light/dark setting, live.
    System,
}

impl ThemeMode {
    /// `Dark` or `Light` (screenshot passes, the old `settings.json`).
    pub fn from_dark(dark: bool) -> Self {
        if dark { Self::Dark } else { Self::Light }
    }

    /// The stored value; anything unknown is the default.
    pub fn parse(value: &str) -> Self {
        match value {
            LIGHT => Self::Light,
            SYSTEM => Self::System,
            _ => Self::Dark,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => DARK,
            Self::Light => LIGHT,
            Self::System => SYSTEM,
        }
    }

    /// Position in the "Tema" control: Escuro, Claro, Sistema.
    pub fn index(self) -> i32 {
        match self {
            Self::Dark => 0,
            Self::Light => 1,
            Self::System => 2,
        }
    }

    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Light,
            2 => Self::System,
            _ => Self::Dark,
        }
    }

    /// Applies the mode to one window's `Theme` global.
    pub fn apply(self, theme: &crate::ui::Theme<'_>) {
        theme.set_follow_system(self == Self::System);
        theme.set_chosen_dark(self != Self::Light);
    }
}

/// Where the dock was left: monitor, edge and height (spec, decision 11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockPlacement {
    /// Monitor name as reported by the OS (`None` when the OS gives none).
    pub monitor: Option<String>,
    pub edge: Edge,
    /// Hub center as a fraction of the monitor height (0 = top, 1 = bottom).
    pub y_fraction: f64,
}

/// What the app needs at startup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub theme: ThemeMode,
    pub dock: Option<DockPlacement>,
}

/// The shape of the old `settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LegacySettings {
    /// Dark theme (the default). `false` = light.
    pub dark_theme: bool,
    pub dock: Option<DockPlacement>,
}

impl Default for LegacySettings {
    fn default() -> Self {
        Self {
            dark_theme: true,
            dock: None,
        }
    }
}

impl Settings {
    /// Reads the settings from the table; a missing or invalid value gives its default.
    pub fn read(core: &AppCore) -> Self {
        let get = |key: &str| match core.setting(key) {
            Ok(value) => value,
            Err(e) => {
                tracing::warn!("cannot read the setting {key}: {e}; using the default");
                None
            }
        };
        let theme = get(keys::THEME).map_or_else(ThemeMode::default, |v| ThemeMode::parse(&v));
        let dock = get(keys::DOCK).and_then(|json| {
            serde_json::from_str(&json)
                .inspect_err(|e| tracing::warn!("invalid dock position {json:?}: {e}"))
                .ok()
        });
        Self { theme, dock }
    }
}

/// Imports the old `settings.json` at `legacy` (when given) into the table, then reads the
/// table. The import is tried once ([`keys::LEGACY_IMPORTED`]) and only while the table has no
/// theme. Screenshot runs pass `None` and never read the user's file.
pub fn load(core: &mut AppCore, legacy: Option<&Path>) -> Settings {
    if let Some(path) = legacy {
        import_legacy(core, path);
    }
    Settings::read(core)
}

fn import_legacy(core: &mut AppCore, path: &Path) {
    match core.setting(keys::LEGACY_IMPORTED) {
        Ok(None) => {}
        Ok(Some(_)) => return,
        Err(e) => return tracing::warn!("cannot check the settings table: {e}"),
    }
    if let Err(e) = core.set_setting(keys::LEGACY_IMPORTED, "1") {
        tracing::warn!("cannot mark the settings import as done: {e}");
    }
    match core.setting(keys::THEME) {
        Ok(None) => {}
        Ok(Some(_)) => return,
        Err(e) => return tracing::warn!("cannot check the settings table: {e}"),
    }
    let Some(old) = read_json(path) else { return };
    let result =
        save_theme(core, ThemeMode::from_dark(old.dark_theme)).and_then(|()| match &old.dock {
            Some(placement) => save_dock(core, placement),
            None => Ok(()),
        });
    match result {
        Ok(()) => tracing::info!("imported {} into the settings table", path.display()),
        Err(e) => tracing::warn!("failed to import {}: {e:#}", path.display()),
    }
}

/// The old file; `None` when it is missing or unreadable (logged).
fn read_json(path: &Path) -> Option<LegacySettings> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!("cannot read {}: {e}", path.display());
            return None;
        }
    };
    serde_json::from_str(&text)
        .inspect_err(|e| tracing::warn!("invalid settings in {}: {e}", path.display()))
        .ok()
}

/// Saves the theme ("Configurações").
pub fn save_theme(core: &mut AppCore, mode: ThemeMode) -> anyhow::Result<()> {
    core.set_setting(keys::THEME, mode.as_str())?;
    Ok(())
}

/// Saves where the dock was left.
pub fn save_dock(core: &mut AppCore, placement: &DockPlacement) -> anyhow::Result<()> {
    core.set_setting(keys::DOCK, &serde_json::to_string(placement)?)?;
    Ok(())
}

/// The old file: `<config dir>/settings.json`.
pub fn default_path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|dir| dir.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use clocky_core::clock::FakeClock;
    use clocky_core::time::Timestamp;

    use super::*;

    fn core() -> AppCore {
        AppCore::in_memory(Box::new(FakeClock::new(Timestamp::from_millis(0))))
    }

    fn placement() -> DockPlacement {
        DockPlacement {
            monitor: Some(r"\\.\DISPLAY1".into()),
            edge: Edge::Left,
            y_fraction: 0.25,
        }
    }

    #[test]
    fn defaults_are_dark_without_dock_position() {
        let s = Settings::read(&core());
        assert_eq!(s, Settings::default());
        assert_eq!(s.theme, ThemeMode::Dark);
    }

    #[test]
    fn theme_and_dock_roundtrip_through_the_table() {
        let mut core = core();
        save_theme(&mut core, ThemeMode::Light).unwrap();
        save_dock(&mut core, &placement()).unwrap();
        assert_eq!(core.setting(keys::THEME).unwrap().as_deref(), Some("light"));
        let s = Settings::read(&core);
        assert_eq!(s.theme, ThemeMode::Light);
        assert_eq!(s.dock, Some(placement()));
        save_theme(&mut core, ThemeMode::System).unwrap();
        assert_eq!(
            core.setting(keys::THEME).unwrap().as_deref(),
            Some("system")
        );
        assert_eq!(Settings::read(&core).theme, ThemeMode::System);
        save_theme(&mut core, ThemeMode::Dark).unwrap();
        assert_eq!(Settings::read(&core).theme, ThemeMode::Dark);
    }

    #[test]
    fn invalid_values_fall_back_to_the_defaults() {
        let mut core = core();
        core.set_setting(keys::THEME, "sepia").unwrap();
        core.set_setting(keys::DOCK, "{ not json").unwrap();
        assert_eq!(Settings::read(&core), Settings::default());
    }

    #[test]
    fn the_old_file_is_imported_once_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let old = LegacySettings {
            dark_theme: false,
            dock: Some(placement()),
        };
        let json = serde_json::to_string_pretty(&old).unwrap();
        fs::write(&path, &json).unwrap();
        let mut core = core();
        assert_eq!(
            load(&mut core, Some(&path)),
            Settings {
                theme: ThemeMode::Light,
                dock: Some(placement()),
            }
        );
        // The file is untouched.
        assert_eq!(fs::read_to_string(&path).unwrap(), json);
        // Later changes win over the file.
        save_theme(&mut core, ThemeMode::Dark).unwrap();
        assert_eq!(load(&mut core, Some(&path)).theme, ThemeMode::Dark);
    }

    #[test]
    fn a_table_with_a_theme_ignores_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"dark_theme": false}"#).unwrap();
        let mut core = core();
        save_theme(&mut core, ThemeMode::Dark).unwrap();
        assert_eq!(load(&mut core, Some(&path)).theme, ThemeMode::Dark);
    }

    #[test]
    fn missing_or_corrupt_old_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = core();
        assert_eq!(
            load(&mut core, Some(&dir.path().join("settings.json"))),
            Settings::default()
        );
        let path = dir.path().join("broken.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&mut core, Some(&path)), Settings::default());
        assert_eq!(core.setting(keys::THEME).unwrap(), None);
    }

    #[test]
    fn a_corrupt_old_file_is_tried_once_and_never_overwrites_newer_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        let mut core = core();
        assert_eq!(load(&mut core, Some(&path)), Settings::default());
        assert_eq!(
            core.setting(keys::LEGACY_IMPORTED).unwrap().as_deref(),
            Some("1")
        );
        // The dock moves; later the old file becomes readable with another position.
        let newer = DockPlacement {
            y_fraction: 0.75,
            ..placement()
        };
        save_dock(&mut core, &newer).unwrap();
        let old = LegacySettings {
            dark_theme: false,
            dock: Some(placement()),
        };
        fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();
        let loaded = load(&mut core, Some(&path));
        assert_eq!(loaded.dock, Some(newer));
        assert_eq!(loaded.theme, ThemeMode::Dark);
    }

    #[test]
    fn missing_fields_of_the_old_file_take_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        let mut core = core();
        assert_eq!(load(&mut core, Some(&path)), Settings::default());
        assert_eq!(core.setting(keys::THEME).unwrap().as_deref(), Some("dark"));
    }

    #[test]
    fn no_legacy_path_reads_only_the_table() {
        let mut core = core();
        assert_eq!(load(&mut core, None), Settings::default());
        assert_eq!(core.setting(keys::THEME).unwrap(), None);
    }

    #[test]
    fn theme_modes_roundtrip_and_unknown_values_are_dark() {
        for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System] {
            assert_eq!(ThemeMode::parse(mode.as_str()), mode);
            assert_eq!(ThemeMode::from_index(mode.index()), mode);
        }
        assert_eq!(ThemeMode::parse("sepia"), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_index(9), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_dark(false), ThemeMode::Light);
    }

    #[test]
    fn edge_is_stored_in_lowercase() {
        let json = serde_json::to_string(&placement()).unwrap();
        assert!(json.contains(r#""edge":"left""#), "{json}");
    }
}
