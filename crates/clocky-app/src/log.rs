//! File logging with `tracing`. The app has no console (`windows_subsystem = "windows"`), so
//! everything, panics included, goes to `clocky.log` in the user data dir.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const LOG_FILE: &str = "clocky.log";
/// At startup, a log bigger than this is moved to `clocky.log.old`.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// The opened log file. A failed rotation does not stop logging: it is reported once the
/// logger runs.
struct OpenedLog {
    file: File,
    path: PathBuf,
    rotation_error: Option<std::io::Error>,
}

/// Starts logging to `<dir>/clocky.log` and routes panics to the log. Returns the log path.
/// Call it only in the primary instance: a second launch must not write to (or rotate) the log
/// of the running one.
///
/// The writer is blocking on purpose: a buffered writer would lose its last lines when the
/// process is killed.
pub fn init(dir: &Path) -> anyhow::Result<PathBuf> {
    let opened = open_log(dir, MAX_LOG_BYTES)?;
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(opened.file))
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to install the logger: {e}"))?;
    install_panic_hook();
    if let Some(e) = opened.rotation_error {
        tracing::warn!("failed to rotate {}: {e}", opened.path.display());
    }
    Ok(opened.path)
}

fn open_log(dir: &Path, max_bytes: u64) -> std::io::Result<OpenedLog> {
    fs::create_dir_all(dir)?;
    let path = dir.join(LOG_FILE);
    let rotation_error = rotate_if_large(&path, max_bytes).err();
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok(OpenedLog {
        file,
        path,
        rotation_error,
    })
}

/// Moves `path` to `<path>.old` when it is larger than `max` bytes.
fn rotate_if_large(path: &Path, max: u64) -> std::io::Result<()> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > max => fs::rename(path, path.with_extension("log.old")),
        _ => Ok(()),
    }
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_log_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        fs::write(&path, "abc").unwrap();
        rotate_if_large(&path, 10).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "abc");
    }

    #[test]
    fn large_log_is_moved_to_old() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        fs::write(&path, "0123456789abc").unwrap();
        rotate_if_large(&path, 10).unwrap();
        assert!(!path.exists());
        let old = dir.path().join("clocky.log.old");
        assert_eq!(fs::read_to_string(old).unwrap(), "0123456789abc");
    }

    #[test]
    fn failed_rotation_still_opens_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        fs::write(&path, "0123456789abc").unwrap();
        // A directory where the rotated file should go makes the rename fail.
        fs::create_dir(dir.path().join("clocky.log.old")).unwrap();
        let opened = open_log(dir.path(), 10).unwrap();
        assert_eq!(opened.path, path);
        assert!(opened.rotation_error.is_some());
        assert!(path.exists());
    }

    #[test]
    fn open_log_creates_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("data").join("logs");
        let opened = open_log(&logs, 10).unwrap();
        assert!(opened.rotation_error.is_none());
        assert!(logs.join(LOG_FILE).exists());
    }

    #[test]
    fn missing_log_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        assert!(rotate_if_large(&dir.path().join(LOG_FILE), 10).is_ok());
    }
}
