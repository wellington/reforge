pub mod docker;
pub mod helm;

use async_trait::async_trait;

use crate::error::Result;

/// A detected dependency in a managed file.
#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub current_version: String,
    pub registry: RegistrySource,
    pub file_path: String,
    pub update_context: UpdateContext,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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
}

#[async_trait]
pub trait PackageManager: Send + Sync {
    fn name(&self) -> &'static str;

    fn file_patterns(&self) -> Vec<&'static str>;

    fn extract_dependencies(&self, file_path: &str, contents: &str) -> Result<Vec<Dependency>>;
}
