//! Package managers for detecting dependencies in different file formats.
//!
//! This module defines the [`PackageManager`] trait and provides implementations
//! for common dependency formats:
//!
//! - [`docker::DockerManager`] — Dockerfiles and docker-compose.yaml
//! - [`helm::HelmManager`] — Chart.yaml and values.yaml
//! - [`regex::RegexManager`] — user-defined regex patterns
//!
//! Each manager extracts [`Dependency`] instances from file contents, which are
//! then passed to the registry layer for version lookup.

pub mod docker;
pub mod helm;
pub mod regex;

use async_trait::async_trait;

use crate::error::Result;

/// A dependency detected in a managed file.
///
/// Contains all information needed to:
/// - Display the dependency to users (name, current version, file path)
/// - Look up available versions (registry source)
/// - Apply updates (update context with precise location information)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// Human-readable name of the dependency (e.g., "nginx", "redis").
    pub name: String,
    /// Current version string as found in the file.
    pub current_version: String,
    /// Where to look up available versions.
    pub registry: RegistrySource,
    /// Path to the file containing this dependency.
    pub file_path: String,
    /// Information needed to update the version in the file.
    pub update_context: UpdateContext,
}

/// Source registry for looking up available versions.
///
/// Different dependency types use different registries and protocols:
/// - Docker images use the Docker Registry v2 API
/// - Helm charts use either HTTP repo index.yaml or OCI registries
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrySource {
    /// A Docker/OCI container image.
    DockerRegistry {
        /// Full image reference (e.g., "library/nginx" or "myorg/myimage").
        image: String,
        /// Optional registry host (e.g., "ghcr.io"). None means Docker Hub.
        registry: Option<String>,
    },
    /// A Helm chart hosted in an HTTP-based chart repository.
    HelmRepository {
        /// Base URL of the repository (e.g., "https://charts.bitnami.com/bitnami").
        repo_url: String,
        /// Name of the chart within the repository.
        chart_name: String,
    },
    /// A Helm chart stored in an OCI registry.
    OciHelmRegistry {
        /// Full OCI image reference for the chart.
        image: String,
        /// Optional registry host. None means Docker Hub.
        registry: Option<String>,
    },
}

/// Context needed to apply a version update to a specific location in a file.
///
/// Different file formats require different update strategies. This enum
/// captures the information needed for each type of dependency reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateContext {
    /// A version value at a YAML key path (e.g., `dependencies[0].version`).
    YamlKeyPath {
        /// Path of keys from root to the version value.
        keys: Vec<String>,
    },
    /// A `FROM` instruction in a Dockerfile.
    DockerFrom {
        /// Zero-based line number of the FROM instruction.
        line_number: usize,
        /// Full image reference including tag (e.g., "nginx:1.25").
        full_reference: String,
    },
    /// An `image:` key in a docker-compose service.
    DockerComposeImage {
        /// Path to the service (e.g., `["services", "web", "image"]`).
        service_path: Vec<String>,
        /// Full image reference including tag.
        full_reference: String,
    },
    /// A match produced by a custom regex manager.
    RegexMatch {
        /// The full text matched by the regex (used to locate the span).
        matched_text: String,
        /// The captured `currentValue` within the match (to be replaced).
        old_value: String,
    },
}

/// Trait for extracting dependencies from configuration files.
///
/// Implementations scan file contents for dependency declarations and return
/// structured [`Dependency`] instances. Each manager handles a specific file
/// format (Dockerfile, Chart.yaml, etc.).
#[async_trait]
pub trait PackageManager: Send + Sync {
    /// Returns the manager's name for logging and branch naming.
    fn name(&self) -> &'static str;

    /// Returns glob patterns for files this manager can process.
    ///
    /// Examples: `["Dockerfile", "Dockerfile.*"]`, `["Chart.yaml", "values.yaml"]`
    fn file_patterns(&self) -> Vec<&'static str>;

    /// Extracts dependencies from file contents.
    ///
    /// # Arguments
    /// * `file_path` - Path to the file (for error messages and context)
    /// * `contents` - The file's text content
    ///
    /// # Returns
    /// A list of dependencies found in the file, or an error if parsing fails.
    fn extract_dependencies(&self, file_path: &str, contents: &str) -> Result<Vec<Dependency>>;
}
