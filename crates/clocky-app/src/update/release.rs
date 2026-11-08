//! What a GitHub release offers: its version, the package for this platform and the checksum
//! list. Pure functions over the API's JSON, so they are unit-tested.
//!
//! Release assets (built by `.github/workflows/release.yml`):
//! - Windows: `clocky-setup-<target>.exe`, the installer (the app itself, which installs or
//!   updates when started with that name or with `--update`).
//! - Linux and macOS: `clocky-v<version>-<target>.tar.gz`, with the `clocky` binary inside.
//! - `SHA256SUMS.txt`: `<sha256>  <file name>` for every asset.

use serde::Deserialize;

/// The checksum list of a release.
pub const CHECKSUMS: &str = "SHA256SUMS.txt";

/// `major.minor.patch`, ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// `0.2.0` or `v0.2.0`; anything after a `-` or `+` (pre-release, build) is ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let core = text.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let version = Self(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }

    /// Builds from development (`0.0.0`) never update: their version is not a real one.
    pub fn is_release(self) -> bool {
        self != Self(0, 0, 0)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// Reads the `releases/latest` answer. Drafts, pre-releases and tags that are not a version
/// are refused.
pub fn parse(json: &str) -> anyhow::Result<Release> {
    let api: ApiRelease = serde_json::from_str(json)?;
    if api.draft || api.prerelease {
        anyhow::bail!("the latest release is a draft or a pre-release");
    }
    let version = Version::parse(&api.tag_name)
        .ok_or_else(|| anyhow::anyhow!("the tag {:?} is not a version", api.tag_name))?;
    Ok(Release {
        version,
        assets: api.assets,
    })
}

/// The package of this platform: the installer on Windows, the archive elsewhere.
pub fn package<'a>(assets: &'a [Asset], target: &str) -> Option<&'a Asset> {
    let windows = target.contains("windows");
    assets.iter().find(|a| {
        if windows {
            a.name == format!("clocky-setup-{target}.exe")
        } else {
            a.name.starts_with("clocky-") && a.name.ends_with(&format!("-{target}.tar.gz"))
        }
    })
}

/// The checksum of `name` in a `SHA256SUMS.txt` (`<hex>  <name>` or `<hex> *<name>`), lower case.
pub fn checksum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: &str = "x86_64-pc-windows-msvc";
    const LINUX: &str = "x86_64-unknown-linux-gnu";

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            url: format!("https://example.com/{name}"),
        }
    }

    #[test]
    fn versions_parse_with_or_without_v_and_compare() {
        assert_eq!(Version::parse("v0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(Version::parse("1.10.3"), Some(Version(1, 10, 3)));
        assert_eq!(Version::parse("v1.0.0-rc.1"), Some(Version(1, 0, 0)));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("latest"), None);
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 0));
        assert!(!Version(0, 0, 0).is_release());
        assert_eq!(Version(0, 2, 1).to_string(), "0.2.1");
    }

    #[test]
    fn the_latest_release_is_read_from_the_api_answer() {
        let json = r#"{"tag_name":"v0.2.0","draft":false,"prerelease":false,
            "assets":[{"name":"SHA256SUMS.txt","browser_download_url":"https://x/s"}],
            "body":"notes"}"#;
        let release = parse(json).unwrap();
        assert_eq!(release.version, Version(0, 2, 0));
        assert_eq!(release.assets[0].name, CHECKSUMS);
    }

    #[test]
    fn drafts_pre_releases_and_odd_tags_are_refused() {
        assert!(parse(r#"{"tag_name":"v0.3.0","prerelease":true}"#).is_err());
        assert!(parse(r#"{"tag_name":"v0.3.0","draft":true}"#).is_err());
        assert!(parse(r#"{"tag_name":"nightly"}"#).is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn each_platform_gets_its_package() {
        let assets = [
            asset("clocky-v0.2.0-x86_64-pc-windows-msvc.zip"),
            asset("clocky-setup-x86_64-pc-windows-msvc.exe"),
            asset("clocky-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"),
            asset("clocky-v0.2.0-aarch64-apple-darwin.tar.gz"),
            asset(CHECKSUMS),
        ];
        assert_eq!(
            package(&assets, WIN).unwrap().name,
            "clocky-setup-x86_64-pc-windows-msvc.exe"
        );
        assert_eq!(
            package(&assets, LINUX).unwrap().name,
            "clocky-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(package(&assets, "riscv64gc-unknown-linux-gnu"), None);
    }

    #[test]
    fn checksums_are_found_by_file_name() {
        let hash = "AB".repeat(32);
        let sums = format!(
            "{}  clocky-v0.2.0-x86_64-unknown-linux-gnu.tar.gz\n{hash} *clocky-setup-x86_64-pc-windows-msvc.exe\n",
            "0".repeat(64)
        );
        assert_eq!(
            checksum(&sums, "clocky-setup-x86_64-pc-windows-msvc.exe"),
            Some("ab".repeat(32))
        );
        assert_eq!(
            checksum(&sums, "clocky-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"),
            Some("0".repeat(64))
        );
        assert_eq!(checksum(&sums, "other.exe"), None);
        assert_eq!(checksum("short  clocky.exe", "clocky.exe"), None);
    }
}
