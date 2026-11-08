//! Installing a downloaded version over the running one.
//!
//! The running Clocky starts the new executable with
//! `--update <target> --wait-pid <pid> [--relaunch]` and quits. The new process (which never
//! touches the single-instance socket) waits for the old one to end, puts a copy of itself at
//! `<target>` and, with `--relaunch`, starts it.
//!
//! - Windows: a running exe cannot be replaced, so the replace is retried until the old
//!   process released it.
//! - Linux and macOS: a rename over a running binary succeeds right away, so the old process
//!   is waited for by its pid (`kill -0`) before the relaunch; otherwise the relaunched app
//!   would hand its launch to the old instance and exit.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::net::hidden;

/// How long the old process gets to quit.
const WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(250);

/// What `--update` was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateArgs {
    /// The executable to replace.
    pub target: PathBuf,
    /// The process that has to end first.
    pub pid: u32,
    /// Start `target` once replaced.
    pub relaunch: bool,
}

impl UpdateArgs {
    /// Parses what follows `--update`: `<target> --wait-pid <pid> [--relaunch]`.
    pub fn parse(args: &[String]) -> Option<Self> {
        match args {
            [target, flag, pid, rest @ ..] if flag == "--wait-pid" => {
                let relaunch = match rest {
                    [] => false,
                    [r] if r == "--relaunch" => true,
                    _ => return None,
                };
                Some(Self {
                    target: PathBuf::from(target),
                    pid: pid.parse().ok()?,
                    relaunch,
                })
            }
            _ => None,
        }
    }

    fn to_args(&self) -> Vec<std::ffi::OsString> {
        let mut args: Vec<std::ffi::OsString> = vec![
            "--update".into(),
            self.target.clone().into(),
            "--wait-pid".into(),
            self.pid.to_string().into(),
        ];
        if self.relaunch {
            args.push("--relaunch".into());
        }
        args
    }
}

/// Starts `new_exe` in update mode for the running executable. The caller quits right after.
pub fn spawn(new_exe: &Path, relaunch: bool) -> anyhow::Result<()> {
    let args = UpdateArgs {
        target: std::env::current_exe()?,
        pid: std::process::id(),
        relaunch,
    };
    tracing::info!("handing over to {} with {args:?}", new_exe.display());
    hidden(Command::new(new_exe)).args(args.to_args()).spawn()?;
    Ok(())
}

/// The `--update` mode: replaces the target with this executable and relaunches it.
pub fn run(args: &UpdateArgs) -> anyhow::Result<()> {
    let me = std::env::current_exe()?;
    tracing::info!(
        "updating {} (waiting for pid {})",
        args.target.display(),
        args.pid
    );
    if !cfg!(windows) {
        wait_for_exit(args.pid, WAIT);
    }
    replace(&me, &args.target, WAIT)?;
    tracing::info!("updated {}", args.target.display());
    if args.relaunch {
        Command::new(&args.target).spawn()?;
    }
    Ok(())
}

/// Copies `src` next to `target` and renames it over `target`, retrying while `target` is in
/// use (Windows) for up to `wait`.
pub fn replace(src: &Path, target: &Path, wait: Duration) -> anyhow::Result<()> {
    let staged = staged_path(target);
    std::fs::copy(src, &staged)?;
    make_executable(&staged)?;
    let mut left = attempts(wait, POLL);
    loop {
        match std::fs::rename(&staged, target) {
            Ok(()) => return Ok(()),
            Err(e) if left > 0 => {
                left -= 1;
                tracing::debug!("{} is still in use: {e}", target.display());
                std::thread::sleep(POLL);
            }
            Err(e) => {
                if let Err(clean) = std::fs::remove_file(&staged) {
                    tracing::warn!("cannot remove {}: {clean}", staged.display());
                }
                return Err(e.into());
            }
        }
    }
}

/// How many polls of `poll` fit in `wait` (the clock is not read: retries are counted).
pub fn attempts(wait: Duration, poll: Duration) -> u128 {
    wait.as_millis() / poll.as_millis().max(1)
}

/// `clocky.exe` → `clocky.exe.new` (same folder, so the rename never crosses drives).
fn staged_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    target.with_file_name(name)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Waits until `pid` is gone (`kill -0` fails), for up to `wait`.
fn wait_for_exit(pid: u32, wait: Duration) {
    for _ in 0..attempts(wait, POLL) {
        let alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success());
        if !alive {
            return;
        }
        std::thread::sleep(POLL);
    }
    tracing::warn!("pid {pid} still runs after {wait:?}; updating anyway");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn update_arguments_roundtrip() {
        let parsed = UpdateArgs::parse(&args(&[
            r"C:\x\clocky.exe",
            "--wait-pid",
            "42",
            "--relaunch",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            UpdateArgs {
                target: PathBuf::from(r"C:\x\clocky.exe"),
                pid: 42,
                relaunch: true,
            }
        );
        let back: Vec<String> = parsed
            .to_args()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(UpdateArgs::parse(&back[1..]), Some(parsed));
        assert_eq!(
            UpdateArgs::parse(&args(&["/x/clocky", "--wait-pid", "7"])).map(|a| a.relaunch),
            Some(false)
        );
    }

    #[test]
    fn bad_update_arguments_are_refused() {
        assert_eq!(UpdateArgs::parse(&args(&["/x/clocky"])), None);
        assert_eq!(UpdateArgs::parse(&args(&["/x", "--wait-pid", "abc"])), None);
        assert_eq!(
            UpdateArgs::parse(&args(&["/x", "--wait-pid", "1", "--bogus"])),
            None
        );
    }

    #[test]
    fn replace_puts_the_new_file_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let (src, target) = (dir.path().join("new.bin"), dir.path().join("clocky.exe"));
        std::fs::write(&src, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        replace(&src, &target, Duration::ZERO).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!staged_path(&target).exists());
    }

    #[test]
    fn replace_gives_up_and_cleans_when_the_target_cannot_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("new.bin");
        std::fs::write(&src, b"new").unwrap();
        // A directory cannot be replaced by a file.
        let target = dir.path().join("busy");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"x").unwrap();
        assert!(replace(&src, &target, Duration::ZERO).is_err());
        assert!(!staged_path(&target).exists());
    }
}
