//! Opening the database at startup. A failure shows [`ErrorWindow`](crate::ui::ErrorWindow)
//! with the path and the error instead of the app (spec, section 7). The file is never touched,
//! except when the saved active timers cannot be restored and the user picks "Descartar timers
//! ativos": then only those timers are deleted (the history stays) and the app opens.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use clocky_core::clock::SystemClock;
use clocky_core::store::StoreError;
use clocky_core::tracker::{TrackerError, TrackerOpenError};
use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;

use crate::core::{AppCore, OpenError, Reopened};
use crate::{paths, screenshot, ui};

/// Why the database could not be opened, ready for the error window.
#[derive(Debug)]
pub struct OpenFailure {
    pub path: String,
    /// User-facing message, in PT-BR.
    pub message: String,
    /// The saved active timers could not be decoded: the opened store, to discard them on
    /// request. `None` for every other failure (I/O, a locked or newer database...).
    pub timers: Option<TrackerOpenError>,
}

/// Opens the database (see [`paths::db`]), creating its directory when needed. A screenshot
/// run without `CLOCKY_DB` gets an empty in-memory database instead of the user's file.
pub fn open_core(screenshot: bool) -> Result<(AppCore, Reopened), OpenFailure> {
    match paths::db(screenshot) {
        paths::Db::File(path) => open_at(&path),
        paths::Db::Memory => {
            tracing::info!("screenshot run without CLOCKY_DB: in-memory database");
            AppCore::open_in_memory(Box::new(SystemClock)).map_err(|e| {
                tracing::error!("cannot open an in-memory database: {e}");
                OpenFailure {
                    path: ":memory:".into(),
                    message: user_message(&e),
                    timers: None,
                }
            })
        }
        paths::Db::Unavailable => {
            tracing::error!("no user data directory for the database");
            Err(OpenFailure {
                path: String::new(),
                message: "O sistema não informou uma pasta de dados para o usuário.".into(),
                timers: None,
            })
        }
    }
}

fn open_at(path: &Path) -> Result<(AppCore, Reopened), OpenFailure> {
    let failure = |message: String| OpenFailure {
        path: path.display().to_string(),
        message,
        timers: None,
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
        let message = user_message(&e);
        let timers = match e {
            OpenError::Timers(e) if can_discard(&e.error) => Some(e),
            _ => None,
        };
        OpenFailure {
            timers,
            ..failure(message)
        }
    })
}

/// The error in PT-BR. SQLite's own text stays in English (it is a technical detail).
fn user_message(error: &OpenError) -> String {
    match error {
        OpenError::Store(e) => store_message(e),
        OpenError::Timers(e) if can_discard(&e.error) => "Os timers que estavam rodando não \
             puderam ser restaurados. O histórico continua guardado: descarte só esses timers \
             para abrir o Clocky."
            .into(),
        OpenError::Timers(e) => store_message_of(&e.error),
    }
}

fn store_message_of(error: &TrackerError) -> String {
    match error {
        TrackerError::Store(e) => store_message(e),
        TrackerError::Engine(_) => "Erro ao abrir o banco.".into(),
    }
}

fn store_message(error: &StoreError) -> String {
    match error {
        StoreError::UnsupportedSchema { found, supported } => format!(
            "O banco foi criado por uma versão mais nova do Clocky (esquema {found}; esta versão \
             lê até o {supported})."
        ),
        StoreError::Sqlite(e) => format!("Erro do SQLite: {e}"),
        other => format!("Erro ao abrir o banco: {other}"),
    }
}

/// Whether a failed restore comes from the saved timers themselves (invalid rows the engine or
/// the decoder refuses), so discarding them fixes it. I/O errors and a locked database do not.
fn can_discard(error: &TrackerError) -> bool {
    match error {
        TrackerError::Engine(_) => true,
        TrackerError::Store(e) => matches!(
            e,
            StoreError::Corrupt { .. } | StoreError::Invalid { .. } | StoreError::OutOfRange(_)
        ),
    }
}

thread_local! {
    /// The error window while it is up, for the commands of a second launch.
    static ERROR_WINDOW: RefCell<Option<slint::Weak<ui::ErrorWindow>>> = const { RefCell::new(None) };
}

/// A command from a second launch while the error window is up (the app does not exist yet):
/// "show" brings the window to the front, "quit" quits. `false` when no error window is up.
pub fn run_command(command: crate::command::Command) -> bool {
    let window = ERROR_WINDOW.with(|cell| cell.borrow().as_ref().and_then(slint::Weak::upgrade));
    let Some(window) = window else {
        return false;
    };
    if command == crate::command::Command::Quit {
        crate::app::quit();
        return true;
    }
    if let Err(e) = window.show() {
        tracing::warn!("failed to show the error window: {e}");
    }
    window.window().with_winit_window(|w| {
        w.set_minimized(false);
        w.focus_window();
    });
    true
}

/// Shows the error window; the caller runs the event loop and keeps the returned window alive.
/// "Descartar timers ativos" (only offered when [`OpenFailure::timers`] is set) deletes the saved
/// timers, opens the core again and hands it to `on_opened` in place of the window. In
/// screenshot mode the window is saved to `<dir>/db-error-<theme>.png` and the app quits.
pub fn show_failure(
    failure: OpenFailure,
    screenshot_dir: Option<PathBuf>,
    on_opened: impl FnOnce(AppCore, Reopened) + 'static,
) -> anyhow::Result<ui::ErrorWindow> {
    let window = ui::ErrorWindow::new()?;
    window.set_path(failure.path.clone().into());
    window.set_message(failure.message.clone().into());
    if failure.timers.is_some() {
        window.set_heading("Não foi possível restaurar os timers ativos".into());
        window.set_hint(
            "Descartar apaga só os timers que estavam rodando quando o Clocky fechou; projetos, \
             tarefas e sessões registradas ficam."
                .into(),
        );
        window.set_can_discard(true);
    }
    let pending = Rc::new(RefCell::new(Some((failure.timers, on_opened))));
    let weak = window.as_weak();
    window.on_discard(move || {
        let Some((Some(timers), on_opened)) = pending.borrow_mut().take() else {
            return;
        };
        let Some(window) = weak.upgrade() else { return };
        match discard_and_open(timers) {
            Ok((core, reopened)) => {
                if let Err(e) = window.hide() {
                    tracing::warn!("failed to hide the error window: {e}");
                }
                ERROR_WINDOW.with(|cell| cell.borrow_mut().take());
                on_opened(core, reopened);
            }
            Err(e) => {
                tracing::error!("cannot discard the active timers: {e}");
                window.set_can_discard(false);
                window.set_message(
                    "Não foi possível descartar os timers ativos. Os detalhes estão no log.".into(),
                );
            }
        }
    });
    window.on_quit(crate::app::quit);
    window.window().on_close_requested(|| {
        crate::app::quit();
        slint::CloseRequestResponse::HideWindow
    });
    window.show()?;
    ERROR_WINDOW.with(|cell| *cell.borrow_mut() = Some(window.as_weak()));
    if let Some(dir) = screenshot_dir {
        let weak = window.as_weak();
        slint::Timer::single_shot(Duration::from_millis(800), move || {
            if let Some(window) = weak.upgrade() {
                screenshot::save_all(&dir, "dark", &[("db-error", window.window())]);
            }
            crate::app::quit();
        });
    }
    Ok(window)
}

/// Deletes the saved active timers (history stays) and opens the core again.
fn discard_and_open(timers: TrackerOpenError) -> anyhow::Result<(AppCore, Reopened)> {
    let store = timers.discard_active_timers()?;
    tracing::warn!("discarded the active timers that could not be restored");
    AppCore::with_store(store, Box::new(SystemClock)).map_err(|e| anyhow::anyhow!("{}", e.error))
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
        let message = store_message(&StoreError::UnsupportedSchema {
            found: 99,
            supported: 1,
        });
        assert!(message.contains("versão mais nova"));
        assert!(message.contains("99"));
    }

    #[test]
    fn only_invalid_saved_timers_can_be_discarded() {
        let corrupt = StoreError::Corrupt {
            what: "active timer",
            value: "{".into(),
        };
        assert!(can_discard(&TrackerError::Store(corrupt)));
        let io = StoreError::Sqlite(rusqlite_busy());
        assert!(!can_discard(&TrackerError::Store(io)));
        let newer = StoreError::UnsupportedSchema {
            found: 9,
            supported: 3,
        };
        assert!(!can_discard(&TrackerError::Store(newer)));
    }

    #[test]
    fn a_corrupt_saved_timer_offers_the_discard_and_reopens_without_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clocky.db");
        {
            let (mut core, _) = AppCore::open(&path).unwrap();
            let now = core.now();
            core.tracker_mut().start_stopwatch(None, now).unwrap();
        }
        corrupt_active_timers(&path);
        let failure = open_at(&path).err().unwrap();
        assert!(failure.message.contains("histórico continua guardado"));
        let timers = failure.timers.expect("the discard is offered");
        let (core, _) = discard_and_open(timers).unwrap();
        assert_eq!(core.tracker().timers().iter().count(), 0);
    }

    fn rusqlite_busy() -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".into()),
        )
    }

    fn corrupt_active_timers(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        // A countdown without a target: no engine timer can be built from it.
        conn.execute(
            "UPDATE active_timers SET kind = 'countdown', target_ms = NULL",
            [],
        )
        .unwrap();
    }
}
