//! Clocky's own installer (Windows) and the other launch modes that never become the running
//! app: the installer, the uninstaller and the update hand-over ([`crate::update::apply`]).
//!
//! The release publishes the app itself as `clocky-setup-<target>.exe`. Started under a name
//! that begins with `clocky-setup` (or with `--install`), it shows the install window instead
//! of the app:
//! - copies itself to `%LOCALAPPDATA%\Programs\Clocky\clocky.exe` (per user, no admin);
//! - adds the Start menu shortcut and the entry of "Aplicativos instalados", whose uninstall
//!   runs `clocky.exe --uninstall`;
//! - optionally starts Clocky with Windows;
//! - quits a running Clocky first and starts the installed one at the end.
//!
//! Uninstalling removes the files, the shortcut and those entries, and keeps the data (the
//! database, `%APPDATA%\Clocky`) and the logs (`%LOCALAPPDATA%\Clocky`).
//!
//! `CLOCKY_SETUP_SANDBOX=<dir>` sends everything to `<dir>` and to a throwaway registry key
//! (tests on a real machine).

#[cfg(windows)]
mod windows;

use std::ffi::OsStr;
use std::path::Path;

use crate::update::apply::UpdateArgs;

/// A launch that is not the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// `--update <target> --wait-pid <pid> [--relaunch]`.
    Update(UpdateArgs),
    /// Started as `clocky-setup*.exe`, or `--install`.
    Install,
    /// `--uninstall` (from "Aplicativos instalados").
    Uninstall,
}

/// The mode of this launch, from its arguments and its file name.
pub fn mode(args: &[String], exe: &Path) -> Option<Mode> {
    match args {
        [flag, rest @ ..] if flag == "--update" => UpdateArgs::parse(rest).map(Mode::Update),
        [flag] if flag == "--install" => Some(Mode::Install),
        [flag] if flag == "--uninstall" => Some(Mode::Uninstall),
        [] if is_setup_name(exe.file_stem()) => Some(Mode::Install),
        _ => None,
    }
}

fn is_setup_name(stem: Option<&OsStr>) -> bool {
    stem.and_then(OsStr::to_str)
        .is_some_and(|s| s.to_ascii_lowercase().starts_with("clocky-setup"))
}

/// Runs a mode to its end (the process exits afterwards).
pub fn run(mode: &Mode) -> anyhow::Result<()> {
    match mode {
        Mode::Update(args) => crate::update::apply::run(args),
        #[cfg(windows)]
        Mode::Install => windows::run_window(false),
        #[cfg(windows)]
        Mode::Uninstall => windows::run_window(true),
        #[cfg(not(windows))]
        Mode::Install | Mode::Uninstall => {
            anyhow::bail!("the installer is for Windows; on this system Clocky updates in place")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_setup_name_installs() {
        let none = args(&[]);
        // Plain file names: `\` is no separator on Linux and macOS.
        let setup = std::env::temp_dir().join("clocky-setup-x86_64-pc-windows-msvc.exe");
        assert_eq!(mode(&none, &setup), Some(Mode::Install));
        assert_eq!(
            mode(&none, Path::new("Clocky-Setup.exe")),
            Some(Mode::Install)
        );
        assert_eq!(mode(&none, &std::env::temp_dir().join("clocky.exe")), None);
        // A second launch of the installer with `--send` is not an install.
        assert_eq!(
            mode(&args(&["--send", "show"]), Path::new("clocky-setup.exe")),
            None
        );
    }

    #[test]
    fn flags_pick_the_mode() {
        let exe = Path::new("clocky.exe");
        assert_eq!(mode(&args(&["--install"]), exe), Some(Mode::Install));
        assert_eq!(mode(&args(&["--uninstall"]), exe), Some(Mode::Uninstall));
        assert!(matches!(
            mode(&args(&["--update", "/x/clocky", "--wait-pid", "3"]), exe),
            Some(Mode::Update(_))
        ));
        assert_eq!(mode(&args(&["--update", "/x/clocky"]), exe), None);
        assert_eq!(mode(&args(&[]), exe), None);
    }
}
