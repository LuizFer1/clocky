//! "Iniciar com o sistema": starts Clocky when the user logs in.
//!
//! - Windows: the value `Clocky` = `"<exe>"` under `HKCU\…\CurrentVersion\Run`.
//! - Linux: an XDG autostart entry, `~/.config/autostart/clocky.desktop`.
//! - macOS: a LaunchAgent, `~/Library/LaunchAgents/app.clocky.plist`.
//!
//! The OS state is the truth: the toggle shows [`Autostart::is_enabled`]. The app holds a
//! `Box<dyn Autostart>`; screenshot runs and tests get [`Fake`], so they never write the real
//! entry. The file content generators and [`FileEntry`] build on every OS, so their tests run
//! everywhere.

use std::cell::Cell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Name of the Run value and of the files.
const NAME: &str = "Clocky";
/// Label of the macOS LaunchAgent (also its file name).
const LAUNCH_AGENT_LABEL: &str = "app.clocky";

/// Turns the start with the system on and off.
pub trait Autostart {
    /// Starts `exe` at login (replacing an older entry).
    fn enable(&self, exe: &Path) -> io::Result<()>;
    /// Removes the entry; nothing to do when there is none.
    fn disable(&self) -> io::Result<()>;
    /// Whether the entry exists.
    fn is_enabled(&self) -> io::Result<bool>;
}

/// Turns autostart on or off for the running executable and returns the new real state.
pub fn set(backend: &dyn Autostart, on: bool) -> io::Result<bool> {
    if on {
        backend.enable(&std::env::current_exe()?)?;
    } else {
        backend.disable()?;
    }
    backend.is_enabled()
}

/// The real entry of this OS; `None` where there is none (or no home directory).
pub fn system() -> Option<Box<dyn Autostart>> {
    #[cfg(windows)]
    {
        Some(Box::new(Registry::new(registry::RUN_KEY)))
    }
    #[cfg(target_os = "linux")]
    {
        let dir = directories::BaseDirs::new()?.config_dir().join("autostart");
        Some(Box::new(FileEntry::desktop(&dir)))
    }
    #[cfg(target_os = "macos")]
    {
        let dir = directories::BaseDirs::new()?
            .home_dir()
            .join("Library")
            .join("LaunchAgents");
        Some(Box::new(FileEntry::launch_agent(&dir)))
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// In memory only (screenshot runs and tests).
#[derive(Debug, Default)]
pub struct Fake {
    enabled: Cell<bool>,
}

impl Autostart for Fake {
    fn enable(&self, _exe: &Path) -> io::Result<()> {
        self.enabled.set(true);
        Ok(())
    }

    fn disable(&self) -> io::Result<()> {
        self.enabled.set(false);
        Ok(())
    }

    fn is_enabled(&self) -> io::Result<bool> {
        Ok(self.enabled.get())
    }
}

/// An entry that is one file (Linux `.desktop`, macOS plist) with content made from the exe.
#[cfg_attr(windows, allow(dead_code))]
pub struct FileEntry {
    path: PathBuf,
    content: fn(&Path) -> String,
}

#[cfg_attr(windows, allow(dead_code))]
impl FileEntry {
    /// `<dir>/clocky.desktop` (XDG autostart).
    pub fn desktop(dir: &Path) -> Self {
        Self {
            path: dir.join("clocky.desktop"),
            content: desktop_entry,
        }
    }

    /// `<dir>/app.clocky.plist` (LaunchAgent).
    pub fn launch_agent(dir: &Path) -> Self {
        Self {
            path: dir.join(format!("{LAUNCH_AGENT_LABEL}.plist")),
            content: launch_agent_plist,
        }
    }
}

impl Autostart for FileEntry {
    fn enable(&self, exe: &Path) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, (self.content)(exe))
    }

    fn disable(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn is_enabled(&self) -> io::Result<bool> {
        self.path.try_exists()
    }
}

/// The XDG autostart entry. `Exec` quotes the path (it may hold spaces) with the escapes of the
/// Desktop Entry spec: `\`, `"`, `` ` `` and `$` inside the quotes, then `\` again for the
/// string value, and `%` doubled (field codes).
#[cfg_attr(windows, allow(dead_code))]
pub fn desktop_entry(exe: &Path) -> String {
    let mut quoted = String::from('"');
    for c in exe.to_string_lossy().chars() {
        match c {
            '\\' => quoted.push_str(r"\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str(r"\\");
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            '\n' | '\r' => quoted.push(' '),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    format!(
        "[Desktop Entry]\nType=Application\nName={NAME}\nComment=Pomodoros, cronômetros, \
         temporizadores e alarmes\nExec={quoted}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

/// The LaunchAgent: runs the exe at login (`RunAtLoad`), without restarting it on exit.
#[cfg_attr(windows, allow(dead_code))]
pub fn launch_agent_plist(exe: &Path) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{LAUNCH_AGENT_LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{exe}</string>\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>KeepAlive</key>\n\t<false/>\n\
         </dict>\n</plist>\n"
    )
}

#[cfg_attr(windows, allow(dead_code))]
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// The Run value: the exe path in quotes (Windows paths cannot hold `"`).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn run_value(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(windows)]
pub use registry::Registry;

#[cfg(windows)]
mod registry {
    use std::io;
    use std::path::Path;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

    use super::{Autostart, NAME, run_value};

    /// The key Windows reads at login.
    pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    /// The value `Clocky` under `HKCU\<subkey>`. The subkey is a parameter so tests can use a
    /// throwaway key instead of the real Run key.
    pub struct Registry {
        subkey: String,
    }

    impl Registry {
        pub fn new(subkey: &str) -> Self {
            Self {
                subkey: subkey.to_owned(),
            }
        }

        fn open(&self, flags: u32) -> io::Result<RegKey> {
            RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(&self.subkey, flags)
        }
    }

    impl Autostart for Registry {
        fn enable(&self, exe: &Path) -> io::Result<()> {
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(&self.subkey)?;
            key.set_value(NAME, &run_value(exe))
        }

        fn disable(&self) -> io::Result<()> {
            let key = match self.open(KEY_SET_VALUE) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                other => other?,
            };
            match key.delete_value(NAME) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }

        fn is_enabled(&self) -> io::Result<bool> {
            let key = match self.open(KEY_READ) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
                other => other?,
            };
            match key.get_value::<String, _>(NAME) {
                Ok(_) => Ok(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests;
