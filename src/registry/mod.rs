//! Registry clients for fetching available versions of dependencies.
//!
//! This module defines the [`RegistryClient`] trait and provides implementations
//! for different registry types:
//!
//! - [`docker::DockerRegistryClient`] — Docker Registry v2 API (Docker Hub, GHCR, etc.)
//! - [`helm::HelmRegistryClient`] — Helm chart repositories and OCI registries
//!
//! The [`VersionInfo`] type pairs a parsed semver version with its original tag
//! string, allowing version comparison while preserving the exact tag for updates.

pub mod docker;
pub mod helm;

use async_trait::async_trait;

use crate::error::Result;
use crate::manager::RegistrySource;

/// A resolved version from a registry.
///
/// Pairs a parsed [`semver::Version`] for comparison with the original tag
/// string for display and file updates (e.g., `v1.25.0` vs `1.25.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// Parsed semver version for comparison and policy evaluation.
    pub version: semver::Version,
    /// Original tag string exactly as it appears in the registry.
    pub original_tag: String,
}

/// Trait for fetching available versions from a package registry.
///
/// Implementations handle authentication, pagination, and API differences
/// between registry types. Results are filtered to only include tags that
/// can be parsed as semver versions.
#[async_trait]
pub trait RegistryClient: Send + Sync {
    /// Fetches all available versions for the given registry source.
    ///
    /// Returns only versions that can be parsed as semver. Non-semver tags
    /// like "latest" or "alpine" are silently filtered out.
    async fn fetch_versions(&self, source: &RegistrySource) -> Result<Vec<VersionInfo>>;
}

/// Parses a version string leniently, handling common variations.
///
/// This function handles:
/// - Leading `v` prefix (e.g., `v1.2.3` → `1.2.3`)
/// - Two-part versions (e.g., `1.25` → `1.25.0`)
/// - Single-part versions (e.g., `7` → `7.0.0`)
///
/// Returns `None` if the string cannot be parsed as a version.
///
/// # Examples
/// ```ignore
/// assert_eq!(parse_version_lenient("v1.2.3"), Some(Version::new(1, 2, 3)));
/// assert_eq!(parse_version_lenient("1.25"), Some(Version::new(1, 25, 0)));
/// assert_eq!(parse_version_lenient("latest"), None);
/// ```
pub fn parse_version_lenient(tag: &str) -> Option<semver::Version> {
    let cleaned = tag.strip_prefix('v').unwrap_or(tag);
    semver::Version::parse(cleaned).ok().or_else(|| {
        let parts: Vec<&str> = cleaned.split('.').collect();
        match parts.len() {
            1 => semver::Version::parse(&format!("{}.0.0", cleaned)).ok(),
            2 => semver::Version::parse(&format!("{}.0", cleaned)).ok(),
            _ => None,
        }
    })
}
