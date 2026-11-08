//! Downloads through the system `curl` (Windows 10 1803+, macOS and the Linux desktops ship
//! it), so the app carries no TLS stack of its own. Runs on a worker thread, never on the UI
//! thread. Also the SHA-256 of a file and the extraction of a `.tar.gz` (through `tar`).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// Total time allowed for one request (the Windows installer is about 10 MB).
const MAX_SECONDS: &str = "600";
/// A generic client name: the GitHub API refuses requests without one.
const USER_AGENT: &str = "Clocky-Updater";

/// A command that opens no console window on Windows (the app has none to share).
pub fn hidden(mut command: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// `curl.exe` from System32 on Windows (never one found first on the `PATH`), `curl` elsewhere.
fn curl_path() -> PathBuf {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        PathBuf::from(root).join("System32").join("curl.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("curl")
    }
}

fn curl(url: &str) -> Command {
    let mut command = hidden(Command::new(curl_path()));
    // -f: HTTP errors fail; -L: GitHub asset links redirect; -sS: quiet but with errors.
    command.args([
        "-fsSL",
        "--max-time",
        MAX_SECONDS,
        "-A",
        USER_AGENT,
        "-H",
        "Accept: application/vnd.github+json",
        url,
    ]);
    command
}

fn run(mut command: Command) -> anyhow::Result<Vec<u8>> {
    let output = command
        .output()
        .map_err(|e| anyhow::anyhow!("curl não está disponível ({e})"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", err.trim().trim_start_matches("curl: "));
    }
    Ok(output.stdout)
}

/// The body of `url` as text.
pub fn fetch_text(url: &str) -> anyhow::Result<String> {
    Ok(String::from_utf8(run(curl(url))?)?)
}

/// Saves `url` to `dest`.
pub fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    let mut command = curl(url);
    command.arg("-o").arg(dest);
    run(command).map(drop)
}

/// SHA-256 of a file, lower-case hex.
pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(buffer.get(..n).unwrap_or_default());
    }
    Ok(hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, b| {
            const DIGITS: &[u8; 16] = b"0123456789abcdef";
            for nibble in [b >> 4, b & 0xf] {
                hex.push(char::from(DIGITS[usize::from(nibble)]));
            }
            hex
        }))
}

/// Extracts a `.tar.gz` into `dir` with the system `tar` (Linux and macOS packages).
pub fn extract_tar_gz(archive: &Path, dir: &Path) -> anyhow::Result<()> {
    let status = hidden(Command::new("tar"))
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dir)
        .status()?;
    anyhow::ensure!(status.success(), "tar failed with {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_a_known_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_missing_file_has_no_checksum() {
        assert!(sha256_file(Path::new("does-not-exist.bin")).is_err());
    }
}
