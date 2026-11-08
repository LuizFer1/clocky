//! The Windows install and uninstall (see the module above). The work runs on a worker thread;
//! the window only shows its progress.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use slint::ComponentHandle;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::autostart::{Autostart, Registry};
use crate::command::Command as AppCommand;
use crate::settings::ThemeMode;
use crate::update::{apply, net::hidden};
use crate::{instance, ui};

/// Sends everything to a folder and a throwaway registry key.
const SANDBOX_ENV: &str = "CLOCKY_SETUP_SANDBOX";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Clocky";
const SANDBOX_KEY: &str = r"Software\Clocky-sandbox";
/// The install folder to remove when the uninstaller ends (it runs from that folder).
static PENDING_REMOVAL: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// How long a running Clocky gets to quit.
const QUIT_WAIT: Duration = Duration::from_secs(15);

/// Where everything goes.
#[derive(Debug, Clone)]
struct Layout {
    dir: PathBuf,
    exe: PathBuf,
    shortcut: PathBuf,
    uninstall_key: String,
    run_key: String,
}

impl Layout {
    fn get() -> anyhow::Result<Self> {
        if let Some(sandbox) = std::env::var_os(SANDBOX_ENV).map(PathBuf::from) {
            return Ok(Self::at(
                &sandbox.join("Programs"),
                &sandbox.join("StartMenu"),
                &format!(r"{SANDBOX_KEY}\Uninstall\Clocky"),
                &format!(r"{SANDBOX_KEY}\Run"),
            ));
        }
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| anyhow::anyhow!("LOCALAPPDATA is not set"))?;
        let roaming =
            std::env::var_os("APPDATA").ok_or_else(|| anyhow::anyhow!("APPDATA is not set"))?;
        Ok(Self::at(
            &PathBuf::from(local).join("Programs"),
            &PathBuf::from(roaming).join(r"Microsoft\Windows\Start Menu\Programs"),
            UNINSTALL_KEY,
            crate::autostart::registry_run_key(),
        ))
    }

    fn at(programs: &Path, start_menu: &Path, uninstall_key: &str, run_key: &str) -> Self {
        let dir = programs.join("Clocky");
        Self {
            exe: dir.join("clocky.exe"),
            dir,
            shortcut: start_menu.join("Clocky.lnk"),
            uninstall_key: uninstall_key.to_owned(),
            run_key: run_key.to_owned(),
        }
    }
}

/// Shows the installer (or the uninstaller) and runs until it closes.
pub fn run_window(uninstall: bool) -> anyhow::Result<()> {
    let layout = Layout::get()?;
    let window = ui::SetupWindow::new()?;
    ThemeMode::System.apply(&window.global::<ui::Theme>());
    window.set_uninstall(uninstall);
    window.set_version(clocky_core::version().into());
    window.set_folder(layout.dir.display().to_string().into());
    window.on_cancel(crate::app::quit);
    let weak = window.as_weak();
    window.on_confirm(move || {
        let Some(w) = weak.upgrade() else { return };
        w.set_busy(true);
        w.set_failed(false);
        w.set_status(
            if uninstall {
                "Desinstalando…"
            } else {
                "Instalando…"
            }
            .into(),
        );
        let (layout, autostart, weak) = (layout.clone(), w.get_autostart(), weak.clone());
        let spawned = std::thread::Builder::new()
            .name("setup".into())
            .spawn(move || {
                let result = if uninstall {
                    uninstall_all(&layout)
                } else {
                    install(&layout, autostart)
                };
                if let Err(e) = &result {
                    tracing::error!("setup failed: {e:#}");
                }
                let shown = weak.upgrade_in_event_loop(move |w| finish(&w, uninstall, result));
                if let Err(e) = shown {
                    tracing::warn!("cannot show the setup result: {e}");
                }
            });
        if let Err(e) = spawned {
            w.set_busy(false);
            w.set_failed(true);
            w.set_status(format!("Não deu para começar: {e}").into());
        }
    });
    window.run()?;
    // The folder of a running uninstaller is removed once its window is gone.
    let pending = PENDING_REMOVAL.lock().ok().and_then(|mut p| p.take());
    if let Some(dir) = pending {
        remove_after_exit(&dir)?;
    }
    Ok(())
}

fn finish(w: &ui::SetupWindow, uninstall: bool, result: anyhow::Result<()>) {
    w.set_busy(false);
    match result {
        Ok(()) => {
            w.set_done(true);
            w.set_status(
                if uninstall {
                    "O Clocky foi desinstalado. Seus dados continuam no computador."
                } else {
                    "O Clocky foi instalado e já está aberto: procure o ícone na borda da tela."
                }
                .into(),
            );
        }
        Err(e) => {
            w.set_failed(true);
            w.set_status(format!("Não deu certo: {e}").into());
        }
    }
}

/// Asks a running Clocky to quit and waits until nobody answers on its name. Never in unit
/// tests: the name is the user's real one.
fn quit_running() {
    if cfg!(test) {
        return;
    }
    let poll = Duration::from_millis(400);
    for _ in 0..apply::attempts(QUIT_WAIT, poll) {
        if !instance::forward(AppCommand::Quit) {
            return;
        }
        std::thread::sleep(poll);
    }
    tracing::warn!("the running Clocky did not quit in {QUIT_WAIT:?}");
}

fn install(layout: &Layout, autostart: bool) -> anyhow::Result<()> {
    quit_running();
    std::fs::create_dir_all(&layout.dir)?;
    let me = std::env::current_exe()?;
    // The old exe may still be closing.
    apply::replace(&me, &layout.exe, QUIT_WAIT)?;
    if let Err(e) = create_shortcut(&layout.shortcut, &layout.exe) {
        // The app works without it.
        tracing::warn!("cannot create the Start menu shortcut: {e:#}");
    }
    register(layout)?;
    if autostart {
        Registry::new(&layout.run_key).enable(&layout.exe)?;
    }
    Command::new(&layout.exe).spawn()?;
    tracing::info!("installed into {}", layout.dir.display());
    Ok(())
}

fn uninstall_all(layout: &Layout) -> anyhow::Result<()> {
    quit_running();
    match std::fs::remove_file(&layout.shortcut) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!("cannot remove the shortcut: {e}");
        }
        _ => {}
    }
    // Only an entry that starts the installed exe (a copy elsewhere keeps its own).
    let run = Registry::new(&layout.run_key);
    if run.starts(&layout.exe)? == Some(true) {
        run.disable()?;
    }
    match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&layout.uninstall_key) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    remove_dir(&layout.dir)?;
    tracing::info!("uninstalled from {}", layout.dir.display());
    Ok(())
}

/// Removes the install folder. The running uninstaller may be the installed exe, which cannot
/// be deleted while it runs: then the folder is removed once its window closes.
fn remove_dir(dir: &Path) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let me = std::env::current_exe()?;
    if !me.starts_with(dir) {
        std::fs::remove_dir_all(dir)?;
        return Ok(());
    }
    if let Ok(mut pending) = PENDING_REMOVAL.lock() {
        *pending = Some(dir.to_owned());
    }
    Ok(())
}

/// `cmd` removes `dir` a moment after this process ends.
fn remove_after_exit(dir: &Path) -> anyhow::Result<()> {
    hidden(Command::new("cmd"))
        .args(["/C", "ping -n 3 127.0.0.1 >nul & rmdir /s /q"])
        // `cmd` reads a `/` as the start of an option.
        .arg(dir.display().to_string().replace('/', "\\"))
        .spawn()?;
    Ok(())
}

/// The Start menu shortcut, through the shell's COM object. The paths go in environment
/// variables, so no quote in a path can break the command.
fn create_shortcut(lnk: &Path, exe: &Path) -> anyhow::Result<()> {
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:CLOCKY_LNK); \
                  $s.TargetPath = $env:CLOCKY_EXE; \
                  $s.WorkingDirectory = $env:CLOCKY_DIR; \
                  $s.Description = 'Clocky'; \
                  $s.Save()";
    let status = hidden(Command::new("powershell.exe"))
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("CLOCKY_LNK", lnk)
        .env("CLOCKY_EXE", exe)
        .env("CLOCKY_DIR", exe.parent().unwrap_or(exe))
        .status()?;
    anyhow::ensure!(status.success(), "powershell failed with {status}");
    Ok(())
}

/// The entry of "Aplicativos instalados".
fn register(layout: &Layout) -> anyhow::Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(&layout.uninstall_key)?;
    let exe = layout.exe.display().to_string();
    key.set_value("DisplayName", &"Clocky")?;
    key.set_value("DisplayVersion", &clocky_core::version())?;
    key.set_value("Publisher", &"Clocky")?;
    key.set_value("DisplayIcon", &format!("{exe},0"))?;
    key.set_value("InstallLocation", &layout.dir.display().to_string())?;
    key.set_value("UninstallString", &format!("\"{exe}\" --uninstall"))?;
    key.set_value("NoModify", &1_u32)?;
    key.set_value("NoRepair", &1_u32)?;
    let size_kb = std::fs::metadata(&layout.exe).map_or(0, |m| m.len() / 1024);
    key.set_value("EstimatedSize", &u32::try_from(size_kb).unwrap_or(u32::MAX))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_puts_the_exe_in_programs_clocky() {
        let l = Layout::at(
            Path::new(r"C:\Users\a\AppData\Local\Programs"),
            Path::new(r"C:\Start"),
            UNINSTALL_KEY,
            "Run",
        );
        assert_eq!(
            l.exe,
            PathBuf::from(r"C:\Users\a\AppData\Local\Programs\Clocky\clocky.exe")
        );
        assert_eq!(l.shortcut, PathBuf::from(r"C:\Start\Clocky.lnk"));
    }

    #[test]
    fn install_and_uninstall_in_a_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let key = format!(r"{SANDBOX_KEY}\test-{}", std::process::id());
        let layout = Layout::at(
            &dir.path().join("Programs"),
            &dir.path().join("Start"),
            &format!(r"{key}\Uninstall"),
            &format!(r"{key}\Run"),
        );
        std::fs::create_dir_all(&layout.dir).unwrap();
        std::fs::write(&layout.exe, b"exe").unwrap();
        register(&layout).unwrap();
        Registry::new(&layout.run_key).enable(&layout.exe).unwrap();
        let entry = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(&layout.uninstall_key)
            .unwrap();
        let uninstall: String = entry.get_value("UninstallString").unwrap();
        assert!(
            uninstall.ends_with("clocky.exe\" --uninstall"),
            "{uninstall}"
        );
        uninstall_all(&layout).unwrap();
        assert!(!layout.dir.exists());
        assert!(
            RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey(&layout.uninstall_key)
                .is_err()
        );
        assert!(!Registry::new(&layout.run_key).is_enabled().unwrap());
        RegKey::predef(HKEY_CURRENT_USER)
            .delete_subkey_all(&key)
            .unwrap();
    }
}
