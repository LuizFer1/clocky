//! Automatic updates from the GitHub releases (Configurações > Atualizações).
//!
//! 30 s after the start and then every 6 h, the latest release is fetched. When it is newer:
//! - "Atualizar automaticamente" on (the default): the package is downloaded, its SHA-256 is
//!   checked against the release's `SHA256SUMS.txt`, and it is installed once the app is idle
//!   (no window open and nothing running, see [`Updater::set_idle_check`]): Clocky hands over
//!   to the new executable ([`apply`]), which replaces it and starts it again.
//! - Off: the page shows the new version and a button to download and install it.
//!
//! The checksum protects against a corrupt or cut download, not against a tampered release
//! (both come from the same place).
//!
//! Builds from development (`0.0.0`) and screenshot runs never update. `CLOCKY_UPDATE_URL`
//! points at another `releases/latest` answer (local tests).

pub mod apply;
pub mod net;
pub mod release;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use release::{Release, Version};

use crate::core::AppCore;

/// The target triple this build runs on (from `build.rs`), to pick the package.
pub const TARGET: &str = env!("CLOCKY_TARGET");
/// Overrides the `releases/latest` URL.
pub const URL_ENV: &str = "CLOCKY_UPDATE_URL";
const LATEST: &str = "https://api.github.com/repos/LuizFer1/clocky/releases/latest";
/// `1` (the default) or `0`.
pub const AUTO_KEY: &str = "update.auto";

const FIRST_CHECK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_hours(6);
/// How often a ready update looks for an idle moment.
const IDLE_POLL: Duration = Duration::from_secs(60);

/// Where the updater is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Development build or screenshot run.
    Disabled,
    /// Not checked yet.
    Idle,
    Checking,
    UpToDate,
    /// Newer and with a package for this platform.
    Available(Release),
    Downloading(Version),
    /// Downloaded and verified: `exe` installs it.
    Ready {
        version: Version,
        exe: PathBuf,
    },
    Failed(String),
}

/// What the Atualizações card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub status: String,
    /// The button; empty = no button.
    pub action: String,
    pub busy: bool,
}

/// The card for a status (pure, tested).
pub fn view(status: &Status, auto: bool) -> View {
    let (status, action, busy) = match status {
        Status::Disabled => (
            "As atualizações automáticas valem para as versões publicadas.".to_owned(),
            "",
            false,
        ),
        Status::Idle => (
            "Ainda não procurou atualizações.".to_owned(),
            "Procurar agora",
            false,
        ),
        Status::Checking => ("Procurando atualizações…".to_owned(), "", true),
        Status::UpToDate => (
            "Você tem a versão mais recente.".to_owned(),
            "Procurar agora",
            false,
        ),
        Status::Available(r) => (
            format!("A versão {} está disponível.", r.version),
            "Baixar e instalar",
            false,
        ),
        Status::Downloading(v) => (format!("Baixando a versão {v}…"), "", true),
        Status::Ready { version, .. } => (
            if auto {
                format!("A versão {version} está pronta e instala quando o Clocky estiver livre.")
            } else {
                format!("A versão {version} está pronta para instalar.")
            },
            "Reiniciar e atualizar",
            false,
        ),
        Status::Failed(e) => (
            format!("Não deu para atualizar: {e}"),
            "Tentar de novo",
            false,
        ),
    };
    View {
        status,
        action: action.to_owned(),
        busy,
    }
}

thread_local! {
    /// The updater, for the results that come back from the worker threads.
    static UPDATER: RefCell<Weak<Updater>> = const { RefCell::new(Weak::new()) };
}

type ViewHook = Box<dyn Fn(&View)>;
type IdleCheck = Box<dyn Fn() -> bool>;

pub struct Updater {
    core: Rc<RefCell<AppCore>>,
    current: Version,
    status: RefCell<Status>,
    auto: Cell<bool>,
    check_timer: slint::Timer,
    idle_timer: slint::Timer,
    on_view: RefCell<Option<ViewHook>>,
    is_idle: RefCell<Option<IdleCheck>>,
}

impl Updater {
    /// `enabled` is false for screenshot runs; a `0.0.0` build is disabled too.
    pub fn new(core: Rc<RefCell<AppCore>>, enabled: bool) -> Rc<Self> {
        let current = Version::parse(clocky_core::version()).unwrap_or(Version(0, 0, 0));
        let auto = core
            .borrow()
            .setting(AUTO_KEY)
            .ok()
            .flatten()
            .is_none_or(|v| v != "0");
        let status = if enabled && current.is_release() {
            Status::Idle
        } else {
            Status::Disabled
        };
        let updater = Rc::new(Self {
            core,
            current,
            status: RefCell::new(status),
            auto: Cell::new(auto),
            check_timer: slint::Timer::default(),
            idle_timer: slint::Timer::default(),
            on_view: RefCell::new(None),
            is_idle: RefCell::new(None),
        });
        UPDATER.with(|cell| *cell.borrow_mut() = Rc::downgrade(&updater));
        updater
    }

    pub fn current(&self) -> Version {
        self.current
    }

    pub fn auto(&self) -> bool {
        self.auto.get()
    }

    /// Called with the card's content after every change.
    pub fn set_on_view(&self, hook: impl Fn(&View) + 'static) {
        *self.on_view.borrow_mut() = Some(Box::new(hook));
        self.publish();
    }

    /// Whether the app may restart now (no window open, nothing running).
    pub fn set_idle_check(&self, check: impl Fn() -> bool + 'static) {
        *self.is_idle.borrow_mut() = Some(Box::new(check));
    }

    /// Schedules the checks (nothing when disabled).
    pub fn start(self: &Rc<Self>) {
        if *self.status.borrow() == Status::Disabled {
            tracing::info!("updates are off for this build ({})", self.current);
            return;
        }
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(FIRST_CHECK, move || {
            if let Some(u) = weak.upgrade() {
                u.check();
            }
        });
        let weak = Rc::downgrade(self);
        self.check_timer
            .start(slint::TimerMode::Repeated, EVERY, move || {
                if let Some(u) = weak.upgrade() {
                    u.check();
                }
            });
        let weak = Rc::downgrade(self);
        self.idle_timer
            .start(slint::TimerMode::Repeated, IDLE_POLL, move || {
                if let Some(u) = weak.upgrade() {
                    u.apply_when_idle();
                }
            });
    }

    /// The card's button.
    pub fn act(&self) {
        let status = self.status.borrow().clone();
        match status {
            Status::Idle | Status::UpToDate | Status::Failed(_) => self.check(),
            Status::Available(release) => self.download(release),
            Status::Ready { exe, .. } => self.apply(&exe),
            Status::Disabled | Status::Checking | Status::Downloading(_) => {}
        }
    }

    /// "Atualizar automaticamente".
    pub fn set_auto(&self, on: bool) {
        self.auto.set(on);
        if let Err(e) = self
            .core
            .borrow_mut()
            .set_setting(AUTO_KEY, if on { "1" } else { "0" })
        {
            tracing::warn!("cannot save the update setting: {e}");
        }
        let available = match &*self.status.borrow() {
            Status::Available(release) if on => Some(release.clone()),
            _ => None,
        };
        if let Some(release) = available {
            self.download(release);
        }
        self.publish();
    }

    fn set(&self, status: Status) {
        tracing::info!("update status: {status:?}");
        *self.status.borrow_mut() = status;
        self.publish();
    }

    fn publish(&self) {
        let view = view(&self.status.borrow(), self.auto.get());
        if let Some(hook) = self.on_view.borrow().as_ref() {
            hook(&view);
        }
    }

    fn check(&self) {
        if matches!(
            *self.status.borrow(),
            Status::Disabled | Status::Checking | Status::Downloading(_) | Status::Ready { .. }
        ) {
            return;
        }
        self.set(Status::Checking);
        let url = std::env::var(URL_ENV).unwrap_or_else(|_| LATEST.to_owned());
        work(
            move || net::fetch_text(&url).and_then(|json| release::parse(&json)),
            Updater::checked,
        );
    }

    fn checked(&self, result: anyhow::Result<Release>) {
        let release = match result {
            Ok(release) => release,
            Err(e) => return self.set(Status::Failed(e.to_string())),
        };
        if release.version <= self.current {
            return self.set(Status::UpToDate);
        }
        if release::package(&release.assets, TARGET).is_none() {
            tracing::warn!("release {} has no package for {TARGET}", release.version);
            return self.set(Status::UpToDate);
        }
        if self.auto.get() {
            self.download(release);
        } else {
            crate::notify::show(
                "Atualização do Clocky",
                &format!(
                    "A versão {} está disponível em Configurações.",
                    release.version
                ),
            );
            self.set(Status::Available(release));
        }
    }

    fn download(&self, release: Release) {
        let version = release.version;
        self.set(Status::Downloading(version));
        let Some(dir) = updates_dir().map(|d| d.join(version.to_string())) else {
            return self.set(Status::Failed("sem pasta para baixar".into()));
        };
        work(
            move || fetch_package(&release, &dir),
            move |u, result| match result {
                Ok(exe) => u.set(Status::Ready { version, exe }),
                Err(e) => u.set(Status::Failed(e.to_string())),
            },
        );
    }

    fn apply_when_idle(&self) {
        let exe = match &*self.status.borrow() {
            Status::Ready { exe, .. } if self.auto.get() => exe.clone(),
            _ => return,
        };
        let idle = self.is_idle.borrow().as_ref().is_some_and(|check| check());
        if idle {
            tracing::info!("the app is idle; installing the update");
            self.apply(&exe);
        }
    }

    /// Hands over to the new executable and quits.
    fn apply(&self, exe: &Path) {
        match apply::spawn(exe, true) {
            Ok(()) => crate::app::quit(),
            Err(e) => self.set(Status::Failed(e.to_string())),
        }
    }
}

/// `<data local dir>/updates`: the downloads (logs live next to it).
fn updates_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "Clocky").map(|d| d.data_local_dir().join("updates"))
}

/// Downloads the package and the checksums into `dir`, verifies the package and returns the
/// executable that installs it.
fn fetch_package(release: &Release, dir: &Path) -> anyhow::Result<PathBuf> {
    let package = release::package(&release.assets, TARGET)
        .ok_or_else(|| anyhow::anyhow!("sem pacote para {TARGET}"))?;
    let sums_asset = release
        .assets
        .iter()
        .find(|a| a.name == release::CHECKSUMS)
        .ok_or_else(|| anyhow::anyhow!("a versão não tem {}", release::CHECKSUMS))?;
    // A leftover from an earlier try is downloaded again.
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    let sums = net::fetch_text(&sums_asset.url)?;
    let expected = release::checksum(&sums, &package.name)
        .ok_or_else(|| anyhow::anyhow!("{} não está em {}", package.name, release::CHECKSUMS))?;
    let file = dir.join(&package.name);
    net::download(&package.url, &file)?;
    let actual = net::sha256_file(&file)?;
    if actual != expected {
        std::fs::remove_file(&file)?;
        anyhow::bail!("o arquivo baixado está corrompido (SHA-256 não confere)");
    }
    if Path::new(&package.name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return Ok(file);
    }
    net::extract_tar_gz(&file, dir)?;
    let exe = dir.join("clocky");
    anyhow::ensure!(exe.exists(), "o pacote não tem o executável clocky");
    Ok(exe)
}

/// Runs `job` on a worker thread and `done` with its result on the UI thread.
fn work<T: Send + 'static>(
    job: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    done: impl FnOnce(&Updater, anyhow::Result<T>) + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            let result = job();
            let delivered = slint::invoke_from_event_loop(move || {
                if let Some(u) = UPDATER.with(|cell| cell.borrow().upgrade()) {
                    done(&u, result);
                }
            });
            if let Err(e) = delivered {
                tracing::warn!("cannot deliver the update result: {e}");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("cannot start the update thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_follows_the_status() {
        let ready = Status::Ready {
            version: Version(0, 3, 0),
            exe: PathBuf::from("x"),
        };
        assert_eq!(view(&ready, true).action, "Reiniciar e atualizar");
        assert!(
            view(&ready, true)
                .status
                .contains("quando o Clocky estiver livre")
        );
        assert!(!view(&ready, false).status.contains("livre"));
        assert!(view(&Status::Checking, true).busy);
        assert_eq!(view(&Status::Checking, true).action, "");
        assert_eq!(view(&Status::Disabled, true).action, "");
        assert_eq!(view(&Status::UpToDate, true).action, "Procurar agora");
        let available = Status::Available(Release {
            version: Version(0, 3, 0),
            assets: Vec::new(),
        });
        assert_eq!(view(&available, false).action, "Baixar e instalar");
        assert!(
            view(&Status::Failed("x".into()), true)
                .status
                .ends_with(": x")
        );
    }

    #[test]
    fn this_build_knows_its_target() {
        assert!(TARGET.contains('-'), "{TARGET}");
    }
}
