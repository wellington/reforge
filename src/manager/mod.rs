pub mod docker;
pub mod helm;
pub mod regex;

use async_trait::async_trait;

use crate::error::Result;

/// A detected dependency in a managed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub current_version: String,
    pub registry: RegistrySource,
    pub file_path: String,
    pub update_context: UpdateContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrySource {
    DockerRegistry {
        image: String,
        registry: Option<String>,
    },
    HelmRepository {
        repo_url: String,
        chart_name: String,
    },
    OciHelmRegistry {
        image: String,
        registry: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UpdateContext {
    YamlKeyPath { keys: Vec<String> },
    DockerFrom {
        line_number: usize,
        full_reference: String,
    },
    DockerComposeImage {
        service_path: Vec<String>,
        full_reference: String,
    },
    /// A match produced by a custom regex manager.
    /// The updater performs a literal string replacement of `old_value` → new version.
    RegexMatch {
        /// The full text that was matched by the regex (used to locate the span).
        matched_text: String,
        /// The captured `currentValue` within that match (to be replaced).
        old_value: String,
    },
}

#[async_trait]
pub trait PackageManager: Send + Sync {
    fn name(&self) -> &'static str;

    fn file_patterns(&self) -> Vec<&'static str>;

    fn extract_dependencies(&self, file_path: &str, contents: &str) -> Result<Vec<Dependency>>;
}
