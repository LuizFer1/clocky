//! Opening the database at startup. A failure shows [`ErrorWindow`](crate::ui::ErrorWindow)
//! with the path and the error instead of the app (spec, section 7). The file is never touched.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clocky_core::store::StoreError;
use slint::ComponentHandle;

use crate::core::AppCore;
use crate::{paths, screenshot, ui};

/// Why the database could not be opened, ready for the error window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFailure {
    pub path: String,
    /// User-facing message, in PT-BR.
    pub message: String,
}

/// Opens the database (see [`paths::db_path`]), creating its directory when needed.
pub fn open_core() -> Result<AppCore, OpenFailure> {
    let Some(path) = paths::db_path() else {
        tracing::error!("no user data directory for the database");
        return Err(OpenFailure {
            path: String::new(),
            message: "O sistema não informou uma pasta de dados para o usuário.".into(),
        });
    };
    open_at(&path)
}

fn open_at(path: &Path) -> Result<AppCore, OpenFailure> {
    let failure = |message: String| OpenFailure {
        path: path.display().to_string(),
        message,
    };
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| {
            tracing::error!(
                "cannot create the database directory {}: {e}",
                dir.display()
            );
            failure(format!("Não foi possível criar a pasta: {e}"))
        })?;
    }
    AppCore::open(path).map_err(|e| {
        // Logged as is (English); the window gets the PT-BR text.
        tracing::error!("cannot open the database {}: {e}", path.display());
        failure(user_message(&e))
    })
}

/// The error in PT-BR. SQLite's own text stays in English (it is a technical detail).
fn user_message(error: &StoreError) -> String {
    match error {
        StoreError::UnsupportedSchema { found, supported } => format!(
            "O banco foi criado por uma versão mais nova do Clocky (esquema {found}; esta versão \
             lê até o {supported})."
        ),
        StoreError::Sqlite(e) => format!("Erro do SQLite: {e}"),
        other => format!("Erro ao abrir o banco: {other}"),
    }
}

/// Shows the error window and runs the event loop until it is closed. In screenshot mode the
/// window is saved to `<dir>/db-error-<theme>.png` and the app quits.
pub fn show_failure(failure: &OpenFailure, screenshot_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let window = ui::ErrorWindow::new()?;
    window.set_path(failure.path.clone().into());
    window.set_message(failure.message.clone().into());
    window.on_quit(crate::app::quit);
    window.window().on_close_requested(|| {
        crate::app::quit();
        slint::CloseRequestResponse::HideWindow
    });
    window.show()?;
    if let Some(dir) = screenshot_dir {
        let weak = window.as_weak();
        slint::Timer::single_shot(Duration::from_millis(800), move || {
            if let Some(window) = weak.upgrade() {
                screenshot::save_all(&dir, "dark", &[("db-error", window.window())]);
            }
            crate::app::quit();
        });
    }
    slint::run_event_loop_until_quit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_the_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("clocky.db");
        assert!(open_at(&path).is_ok());
        assert!(path.exists());
    }

    #[test]
    fn a_directory_is_reported_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let failure = open_at(dir.path()).err().unwrap();
        assert_eq!(failure.path, dir.path().display().to_string());
        assert!(
            failure.message.starts_with("Erro do SQLite"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn a_newer_schema_is_explained_in_portuguese() {
        let message = user_message(&StoreError::UnsupportedSchema {
            found: 99,
            supported: 1,
        });
        assert!(message.contains("versão mais nova"));
        assert!(message.contains("99"));
    }
}
