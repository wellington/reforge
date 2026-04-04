use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{ReforgeError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub gitlab: GitLabConfig,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub managers: ManagersConfig,
    #[serde(default)]
    pub versioning: VersioningConfig,
    #[serde(default)]
    pub merge_request: MergeRequestConfig,
    #[serde(default)]
    pub registries: HashMap<String, RegistryCredential>,
    #[serde(default)]
    pub dashboard: DashboardConfig,
    /// When set, operate against a local git checkout instead of the GitLab API.
    #[serde(default)]
    pub local_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitLabConfig {
    #[serde(default = "default_gitlab_url")]
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
}

fn default_gitlab_url() -> String {
    "https://gitlab.com".to_string()
}

impl Default for GitLabConfig {
    fn default() -> Self {
        Self {
            url: default_gitlab_url(),
            token: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScanConfig {
    #[serde(default)]
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManagersConfig {
    #[serde(default = "default_managers")]
    pub enabled: Vec<String>,
}

impl Default for ManagersConfig {
    fn default() -> Self {
        Self {
            enabled: default_managers(),
        }
    }
}

fn default_managers() -> Vec<String> {
    vec!["helm".to_string(), "docker".to_string()]
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersioningConfig {
    #[serde(default = "default_pin_strategy")]
    pub pin_strategy: String,
}

impl Default for VersioningConfig {
    fn default() -> Self {
        Self {
            pin_strategy: default_pin_strategy(),
        }
    }
}

fn default_pin_strategy() -> String {
    "semver-minor".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct MergeRequestConfig {
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default = "default_grouping")]
    pub grouping: String,
    #[serde(default)]
    pub assignees: Vec<u64>,
    #[serde(default)]
    pub auto_merge: bool,
}

impl Default for MergeRequestConfig {
    fn default() -> Self {
        Self {
            branch_prefix: default_branch_prefix(),
            labels: vec!["reforge".to_string(), "automated".to_string()],
            grouping: default_grouping(),
            assignees: vec![],
            auto_merge: false,
        }
    }
}

fn default_branch_prefix() -> String {
    "reforge/".to_string()
}

fn default_grouping() -> String {
    "per-dependency".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct DashboardConfig {
    /// Whether to create/update a dashboard issue.
    #[serde(default = "default_dashboard_enabled")]
    pub enabled: bool,
    /// Labels to apply to the dashboard issue.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Local path to write the dashboard file when in local mode.
    #[serde(default = "default_dashboard_local_path")]
    pub local_path: String,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: default_dashboard_enabled(),
            labels: vec!["reforge".to_string(), "dependency-dashboard".to_string()],
            local_path: default_dashboard_local_path(),
        }
    }
}

fn default_dashboard_enabled() -> bool {
    true
}

fn default_dashboard_local_path() -> String {
    "DEPENDENCY_DASHBOARD.md".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryCredential {
    pub username: Option<String>,
    pub password_env: Option<String>,
}

impl RegistryCredential {
    pub fn resolve_password(&self) -> Option<String> {
        self.password_env
            .as_ref()
            .and_then(|env_var| std::env::var(env_var).ok())
    }
}

impl Config {
    pub fn load(path: &Path, cli_overrides: CliOverrides) -> Result<Self> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            ReforgeError::Config(format!("Failed to read config file {:?}: {}", path, e))
        })?;

        let mut config: Config = toml::from_str(&contents)
            .map_err(|e| ReforgeError::Config(format!("Invalid config: {}", e)))?;

        // Layer env vars (REFORGE_ preferred, RENOVATE_ accepted for migration)
        if config.gitlab.token.is_none() {
            config.gitlab.token = std::env::var("REFORGE_TOKEN")
                .or_else(|_| std::env::var("RENOVATE_TOKEN"))
                .ok();
        }

        if let Ok(url) = std::env::var("REFORGE_GITLAB_URL")
            .or_else(|_| std::env::var("RENOVATE_GITLAB_URL"))
        {
            config.gitlab.url = url;
        }

        // Layer CLI overrides
        if let Some(token) = cli_overrides.token {
            config.gitlab.token = Some(token);
        }
        if let Some(url) = cli_overrides.gitlab_url {
            config.gitlab.url = url;
        }
        if let Some(repo) = cli_overrides.repo {
            config.scan.projects = vec![repo];
        }
        if let Some(local_path) = cli_overrides.local_path {
            config.local_path = Some(local_path);
        }

        Ok(config)
    }

    /// Build a minimal config purely from CLI args and env vars (no config file).
    pub fn from_cli(cli_overrides: CliOverrides) -> Result<Self> {
        let local_path = cli_overrides.local_path;

        // GitLab URL is optional when in local mode
        let url = cli_overrides
            .gitlab_url
            .or_else(|| std::env::var("REFORGE_GITLAB_URL").ok())
            .or_else(|| std::env::var("RENOVATE_GITLAB_URL").ok())
            .unwrap_or_else(default_gitlab_url);

        let token = cli_overrides
            .token
            .or_else(|| std::env::var("REFORGE_TOKEN").ok())
            .or_else(|| std::env::var("RENOVATE_TOKEN").ok());

        let projects = match cli_overrides.repo {
            Some(r) => vec![r],
            None => vec![],
        };

        Ok(Config {
            gitlab: GitLabConfig { url, token },
            scan: ScanConfig { projects },
            managers: ManagersConfig::default(),
            versioning: VersioningConfig::default(),
            merge_request: MergeRequestConfig::default(),
            registries: HashMap::new(),
            dashboard: DashboardConfig::default(),
            local_path,
        })
    }
}

#[derive(Debug, Default)]
pub struct CliOverrides {
    pub token: Option<String>,
    pub gitlab_url: Option<String>,
    pub repo: Option<String>,
    pub local_path: Option<PathBuf>,
}
