//! App-side settings in `settings.json` in the user config dir. A stopgap until the settings
//! table of M2 exists: it only keeps the theme and the dock position.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::geometry::Edge;

/// Where the dock was left: monitor, edge and height (spec, decision 11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockPlacement {
    /// Monitor name as reported by the OS (`None` when the OS gives none).
    pub monitor: Option<String>,
    pub edge: Edge,
    /// Hub center as a fraction of the monitor height (0 = top, 1 = bottom).
    pub y_fraction: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Dark theme (the default, spec decision 10). `false` = light.
    pub dark_theme: bool,
    pub dock: Option<DockPlacement>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_theme: true,
            dock: None,
        }
    }
}

impl Settings {
    /// Reads the settings. A missing or unreadable file gives the defaults (logged as a warning).
    pub fn load(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                tracing::warn!("cannot read {}: {e}; using defaults", path.display());
                return Self::default();
            }
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(
                "invalid settings in {}: {e}; using defaults",
                path.display()
            );
            Self::default()
        })
    }

    /// Writes the settings through a temporary file, so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Default location: `<config dir>/settings.json`.
pub fn default_path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|dir| dir.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement() -> DockPlacement {
        DockPlacement {
            monitor: Some(r"\\.\DISPLAY1".into()),
            edge: Edge::Left,
            y_fraction: 0.25,
        }
    }

    #[test]
    fn defaults_are_dark_without_dock_position() {
        let s = Settings::default();
        assert!(s.dark_theme);
        assert_eq!(s.dock, None);
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.json");
        let s = Settings {
            dark_theme: false,
            dock: Some(placement()),
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load(&dir.path().join("settings.json")),
            Settings::default()
        );
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn missing_fields_take_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn edge_is_stored_in_lowercase() {
        let json = serde_json::to_string(&placement()).unwrap();
        assert!(json.contains(r#""edge":"left""#), "{json}");
    }
}
