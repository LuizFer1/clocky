# Clocky

A **lightweight, intuitive and pleasant-looking** time management app, written 100% in Rust (UI built with [Slint](https://slint.dev)).

> **Status:** under active development. Releases are published on the
> [GitHub Releases](https://github.com/LuizFer1/clocky/releases) page.

## Features

- Pomodoros, stopwatches, countdown timers and alarms, with several instances running at the same time
- Tasks and subtasks linked to timers and organized into projects
- Time reports: overall, weekly, daily (timeline) and per session
- Desktop app for Windows, Linux and macOS, with a system tray icon
- Light, dark or system theme
- Automatic updates from the GitHub releases

## Installation

**Windows:** download `clocky-setup-x86_64-pc-windows-msvc.exe` from the
[latest release](https://github.com/LuizFer1/clocky/releases/latest) and run it. It installs
Clocky for the current user (no administrator rights) in `%LOCALAPPDATA%\Programs\Clocky`, adds
a Start menu shortcut and an entry in *Installed apps* to uninstall it. Uninstalling keeps your
data.

**Linux and macOS:** download the `.tar.gz` for your system, extract it and run `clocky`.

### Updates

Clocky checks the latest GitHub release 30 seconds after it starts and every 6 hours. With
*Settings → Updates → Update automatically* on (the default), it downloads the new version,
checks it against the release's `SHA256SUMS.txt` and installs it when no window is open and no
timer is running, then starts again. With it off, the page shows the new version and a button to
install it. On Linux and macOS the binary is replaced in place.

> **Coming from v0.1.0:** that version has no updater. Install v0.2.0 by hand once (on Windows,
> with the installer); later versions update themselves.

---

## Development

### Requirements

- Rust (the version is pinned in `rust-toolchain.toml`, and `rustup` installs it automatically)
- Quality gate tools:

```sh
cargo install --locked just cargo-deny cargo-llvm-cov cargo-machete git-cliff
```

### Getting started

```sh
just setup          # enables the project's git hooks (once after cloning)
cargo run -p clocky-app
```

### Running the app

- **Linux build dependencies:** `sudo apt-get install libasound2-dev libfontconfig1-dev` (ALSA for sound, fontconfig for Slint).
- **Linux display server:** the floating icon must be placed at a screen edge, which Wayland does not allow. Clocky therefore runs through X11 whenever `DISPLAY` is set (XWayland on Wayland desktops).
- **Files:** settings (theme, dock position, pomodoro defaults) live in the database's `settings` table; an older `settings.json` in the user config dir is imported once. The log is `clocky.log` in the user data dir (`%LOCALAPPDATA%\Clocky\data\logs` on Windows).
- **Start with the system:** the Configurações toggle writes the per-user entry (Windows `HKCU\...\Run`, Linux `~/.config/autostart/clocky.desktop`, macOS `~/Library/LaunchAgents`).
- **Single instance:** launching Clocky again brings the running one to the front. Debug commands go through the same channel: `clocky --send notify-test`, `clocky --send sound-test` and `clocky --send quit`.
- **Database:** `clocky.db` in the user data dir (`%APPDATA%\Clocky\data` on Windows), or the file named by `CLOCKY_DB`. If it cannot be opened or migrated, Clocky shows the path and the error and does not start; the file is never overwritten.
- **Screenshots:** `CLOCKY_SCREENSHOT_DIR=<dir> clocky` walks through the windows (dock, panels, manage pages, task picker), saves `<dir>/<step>-<theme>.png` in the dark and light themes, and exits. It only renders Clocky's own windows, never the desktop. Add `CLOCKY_DB=<tmp>/demo.db CLOCKY_DEMO=1` to fill an empty database with demo projects first (only when `CLOCKY_DB` is set).

### Project structure

| Crate | Role |
|---|---|
| `crates/clocky-core` | Domain, timer engine, alarms, SQLite and reports. **No UI and no OS APIs.** Fully testable |
| `crates/clocky-app` | Slint UI and integrations (tray, notifications, sound, autostart) |

---

## Quality gate

Nothing is merged without passing the gate:

```sh
just check
```

| Step | Command | Fails if |
|---|---|---|
| Formatting | `just fmt-check` | Any file is not `rustfmt`-formatted |
| Lint | `just lint` | Any clippy warning (`pedantic` enabled) |
| Tests | `just test` | Any test fails |
| Coverage | `just coverage` | `clocky-core` has less than **85%** line coverage |
| Dependencies | `just deny` | License incompatible with GPLv3, known vulnerability or banned crate |
| Unused deps | `just machete` | A dependency is declared but unused |

**Key rules:**

- Forbidden outside tests: `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `dbg!`, `println!`/`eprintln!` and `unsafe`.
- Functions have at most **60 lines**, **5 arguments** and a cognitive complexity of **15**.
- `clocky-core` **does not read the clock directly**: it uses the `Clock` trait. `SystemTime::now` and `Instant::now` are blocked by clippy.
- `clocky-core` **must not depend** on `slint`, `tray-icon`, `notify-rust`, `rodio` or `auto-launch` (enforced by `cargo-deny`).
- Errors: `thiserror` in the core, and `anyhow` only at the app boundary.
- TDD in the core: every fixed bug gets a regression test.

**Local hooks** (enabled with `just setup`):

- `pre-commit`: runs `fmt` and `clippy`.
- `commit-msg`: validates Conventional Commits.

CI runs `just check` on **Windows, Linux and macOS** for every push and PR to `dev` and `main`.

---

## Branching model

```
feat/xyz ──PR──▶ dev ──PR (merge commit)──▶ main ──▶ automatic release
```

| Branch | Role |
|---|---|
| `main` | Released code. Every merge that brings relevant changes triggers a release |
| `dev` | Integration. All work lands here via PR |
| `feat/…`, `fix/…`, `chore/…` | Day-to-day work, branched from `dev` |

- PRs into `dev`: squash or rebase, with a Conventional Commits title.
- PR `dev` → `main`: **merge commit** (no squash), to preserve the commits that determine the version.

### Commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(pomodoro): allow multiple simultaneous pomodoros
fix(alarm): snooze did not survive an app restart
feat(report)!: new session format in the database
```

Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `style`, `build`, `ci`, `chore`, `revert`.

---

## Versioning and releases

The version is calculated automatically from the commits since the last release:

| Commits since the last version | Example | New version |
|---|---|---|
| Breaking change (`type!:` or a `BREAKING CHANGE:` footer) | `feat!: new data format` | `x.0.0` (e.g. `0.4.2` → `1.0.0`) |
| New feature (`feat`) | `feat(alarm): custom sound` | `0.x.0` (e.g. `0.4.2` → `0.5.0`) |
| Fix or performance (`fix`, `perf`) | `fix: countdown pause` | `0.0.x` (e.g. `0.4.2` → `0.4.3`) |
| Only `chore`, `docs`, `ci`, `test`, `refactor`… | | No release |

> Note: a `!` commit bumps to the next **major** version even during the `0.x` phase. For example, the first `feat!:` publishes `1.0.0`.

**How a release happens:**

1. Open a PR from `dev` to `main` and merge it with a **merge commit**.
2. CI runs on `main`. If it passes, the **Release** pipeline:
   - calculates the version (`git-cliff`, rules in `cliff.toml`);
   - builds the binary for Windows (x86_64), Linux (x86_64) and macOS (Apple Silicon);
   - publishes the Windows installer as `clocky-setup-<target>.exe` (the app itself: started
     under that name it installs, and the updater looks for that name) and a `SHA256SUMS.txt`
     with the checksum of every file;
   - creates the `vX.Y.Z` tag and the **GitHub Release** with the binaries and notes generated from the commits.

To see locally what the next version would be:

```sh
just next-version
```

The version in `Cargo.toml` stays at `0.0.0` in the repository. The pipeline writes the real version only during the release build.

---

## License

[GPLv3](LICENSE). Clocky uses [Slint](https://slint.dev) under the GPLv3 license.

Embedded assets: the [Geist](https://github.com/vercel/geist-font) fonts are under the SIL Open Font License 1.1 (`crates/clocky-app/fonts/OFL.txt`), and the icons are covered by `crates/clocky-app/icons/LICENSE.md`.
