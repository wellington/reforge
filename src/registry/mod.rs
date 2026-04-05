pub mod docker;
pub mod helm;

use async_trait::async_trait;

use crate::error::Result;
use crate::manager::RegistrySource;

/// A resolved version from a registry, preserving the original tag string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: semver::Version,
    pub original_tag: String,
}

#[async_trait]
pub trait RegistryClient: Send + Sync {
    async fn fetch_versions(&self, source: &RegistrySource) -> Result<Vec<VersionInfo>>;
}

/// Parse a version string, stripping a leading 'v' if present.
pub fn parse_version_lenient(tag: &str) -> Option<semver::Version> {
    let cleaned = tag.strip_prefix('v').unwrap_or(tag);
    semver::Version::parse(cleaned).ok().or_else(|| {
        // Try appending .0 for two-part versions like "1.25"
        let parts: Vec<&str> = cleaned.split('.').collect();
        match parts.len() {
            1 => semver::Version::parse(&format!("{}.0.0", cleaned)).ok(),
            2 => semver::Version::parse(&format!("{}.0", cleaned)).ok(),
            _ => None,
        }
    })
}
