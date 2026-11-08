//! Clocky core: domain, timer engine, alarms, storage and reports.
//!
//! This crate depends on neither a UI nor operating system APIs.
#![warn(missing_docs)]

pub mod clock;
pub mod domain;
pub mod engine;
pub mod time;

#[cfg(test)]
mod test_util;

/// Clocky version, set by the release pipeline.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        let parts: Vec<&str> = version().split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()));
    }
}
