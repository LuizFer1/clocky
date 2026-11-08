//! Test helpers.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::engine::run::Segment;
use crate::engine::session::{CompletedSession, TimerKind};
use crate::time::Timestamp;

/// The instant `seconds` seconds after the Unix epoch.
pub(crate) fn at(seconds: i64) -> Timestamp {
    Timestamp::from_millis(seconds * 1000)
}

/// Duration of `n` seconds.
pub(crate) fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// Duration of `n` minutes.
pub(crate) fn mins(n: u64) -> Duration {
    Duration::from_mins(n)
}

/// A database file in the system temp folder, unique per test name and process.
/// The database, its WAL files and its backup are removed on creation and on drop.
pub(crate) struct TempDb {
    path: PathBuf,
}

impl TempDb {
    pub(crate) fn new(name: &str) -> Self {
        let file = format!("clocky-test-{}-{name}.db", std::process::id());
        let db = Self {
            path: std::env::temp_dir().join(file),
        };
        db.remove_files();
        db
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn remove_files(&self) {
        for suffix in ["", "-wal", "-shm", ".bak"] {
            let mut name = self.path.as_os_str().to_owned();
            name.push(suffix);
            if let Err(error) = fs::remove_file(&name)
                && error.kind() != ErrorKind::NotFound
            {
                panic!("cannot remove {}: {error}", PathBuf::from(name).display());
            }
        }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        self.remove_files();
    }
}

/// A completed session with one segment per `(start, end)` pair, in seconds.
pub(crate) fn completed(kind: TimerKind, spans: &[(i64, i64)]) -> CompletedSession {
    let segments = spans
        .iter()
        .map(|&(start, end)| Segment {
            start: at(start),
            end: at(end),
        })
        .collect();
    CompletedSession::new(kind, segments).expect("the session has time")
}
