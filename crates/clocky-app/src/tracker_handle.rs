//! The app's handle to the core: opens the database and owns the [`Tracker`] and the clock.
//!
//! Merge note (M6a): M6a's `AppCore` (`core.rs`) owns the `Store` and is meant to switch to the
//! tracker. On merge, `AppCore` takes the tracker from here (same `CLOCKY_DB` variable, same
//! `clocky.db` path in the user data dir) and this file goes away; the timer panels only need
//! `now()`, `tracker()` and `tracker_mut()`.
//!
//! Lives on the UI thread as `Rc<RefCell<TrackerHandle>>`. Callers take short borrows and never
//! hold one across a call into Slint.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use clocky_core::clock::{Clock, SystemClock};
use clocky_core::engine::timers::ActiveTimer;
use clocky_core::store::Store;
use clocky_core::time::Timestamp;
use clocky_core::tracker::{Tracker, TrackerEvent};
use directories::ProjectDirs;

/// Environment variable that points the app at another database file (tests, screenshots).
pub const DB_ENV_VAR: &str = "CLOCKY_DB";

pub struct TrackerHandle {
    tracker: Tracker,
    clock: Box<dyn Clock>,
}

/// An opened tracker with what the reopen tick did.
pub struct Opened {
    pub handle: TrackerHandle,
    /// Events of the reopen tick (what happened while the app was closed).
    pub events: Vec<TrackerEvent>,
    /// The saved timers before that tick: a countdown that came due leaves the set, and the
    /// app still needs its name and task to say so.
    pub before: Vec<ActiveTimer>,
}

impl TrackerHandle {
    /// Opens the database at [`db_path`] with the system clock, creating its directory.
    pub fn open_default() -> anyhow::Result<Opened> {
        let path = db_path().context("the OS gives no user data directory")?;
        Self::open_at(&path, Box::new(SystemClock))
    }

    pub fn open_at(path: &Path, clock: Box<dyn Clock>) -> anyhow::Result<Opened> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let store = Store::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        tracing::info!("database: {}", path.display());
        Self::with_store(store, clock)
    }

    pub fn with_store(store: Store, clock: Box<dyn Clock>) -> anyhow::Result<Opened> {
        let (_, before) = store.load_active()?;
        let (tracker, events) = Tracker::open(store, clock.now())?;
        Ok(Opened {
            handle: Self { tracker, clock },
            events,
            before,
        })
    }

    /// The current instant, from the core's clock (the app never reads the clock elsewhere).
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn tracker(&self) -> &Tracker {
        &self.tracker
    }

    pub fn tracker_mut(&mut self) -> &mut Tracker {
        &mut self.tracker
    }
}

/// Path of the database: `CLOCKY_DB` when set and not empty, otherwise `clocky.db` in the user
/// data dir. `None` when neither is available.
pub fn db_path() -> Option<PathBuf> {
    db_path_from(
        std::env::var_os(DB_ENV_VAR).map(PathBuf::from),
        ProjectDirs::from("", "", "Clocky").map(|dirs| dirs.data_dir().to_path_buf()),
    )
}

fn db_path_from(env: Option<PathBuf>, data_dir: Option<PathBuf>) -> Option<PathBuf> {
    env.filter(|path| !path.as_os_str().is_empty())
        .or_else(|| data_dir.map(|dir| dir.join("clocky.db")))
}

#[cfg(test)]
mod tests {
    use clocky_core::clock::FakeClock;

    use super::*;

    #[test]
    fn env_var_wins_over_the_data_dir() {
        let path = db_path_from(Some("x/test.db".into()), Some("data".into()));
        assert_eq!(path, Some(PathBuf::from("x/test.db")));
    }

    #[test]
    fn data_dir_is_the_default_and_empty_env_is_unset() {
        let path = db_path_from(None, Some("data".into()));
        assert_eq!(path, Some(PathBuf::from("data").join("clocky.db")));
        assert_eq!(
            db_path_from(Some(PathBuf::new()), Some("data".into())),
            path
        );
        assert_eq!(db_path_from(None, None), None);
    }

    #[test]
    fn timers_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("clocky.db");
        let clock = || Box::new(FakeClock::new(Timestamp::from_millis(1_000)));
        let opened = TrackerHandle::open_at(&path, clock()).unwrap();
        assert!(opened.events.is_empty() && opened.before.is_empty());
        let mut handle = opened.handle;
        let now = handle.now();
        handle.tracker_mut().start_stopwatch(None, now).unwrap();
        drop(handle);
        let opened = TrackerHandle::open_at(&path, clock()).unwrap();
        assert_eq!(opened.handle.tracker().timers().iter().count(), 1);
        assert_eq!(opened.before.len(), 1);
    }

    #[test]
    fn opening_a_directory_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(TrackerHandle::open_at(dir.path(), Box::new(SystemClock)).is_err());
    }
}
