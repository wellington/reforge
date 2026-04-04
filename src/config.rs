use regex::Regex;
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
    #[serde(default)]
    pub changelog: ChangelogConfig,
    /// When set, operate against a local git checkout instead of the GitLab API.
    #[serde(default)]
    pub local_path: Option<PathBuf>,
    /// Custom regex-based managers.
    #[serde(default)]
    pub regex_managers: Vec<RegexManagerConfig>,
}

/// Configuration for a single custom regex manager.
#[derive(Debug, Clone, Deserialize)]
pub struct RegexManagerConfig {
    /// Human-readable name for this manager (used in logs and branch names).
    pub name: String,
    /// Glob-style file patterns this manager should match (e.g. `["*.yaml"]`).
    pub file_patterns: Vec<String>,
    /// Regex with named capture groups: `depName`, `currentValue`.
    /// Optional groups: `registryUrl`, `datasource`.
    pub match_pattern: String,
    /// Datasource to use when looking up versions: `docker`, `helm-oci`, or `helm-repo`.
    pub datasource: String,
    /// Registry URL override (used when the regex does not capture `registryUrl`).
    #[serde(default)]
    pub registry_url: Option<String>,
    /// Versioning scheme override (currently informational).
    #[serde(default)]
    pub versioning: Option<String>,
}

impl RegexManagerConfig {
    /// Validate the config: compile the regex and check for required named groups.
    pub fn validate(&self) -> Result<()> {
        let re = Regex::new(&self.match_pattern).map_err(|e| {
            ReforgeError::Config(format!(
                "regex_manager '{}': invalid match_pattern: {}",
                self.name, e
            ))
        })?;

        let names: Vec<&str> = re.capture_names().flatten().collect();
        for required in &["depName", "currentValue"] {
            if !names.contains(required) {
                return Err(ReforgeError::Config(format!(
                    "regex_manager '{}': match_pattern is missing required named capture group '(?P<{}>...)'",
                    self.name, required
                )));
            }
        }

        match self.datasource.as_str() {
            "docker" | "helm-oci" | "helm-repo" => {}
            other => {
                return Err(ReforgeError::Config(format!(
                    "regex_manager '{}': unknown datasource '{}'; expected docker, helm-oci, or helm-repo",
                    self.name, other
                )));
            }
        }

        Ok(())
    }
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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateTypeFilter {
    Patch,
    Minor,
    Major,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AutomergePolicy {
    /// Glob pattern matched against dependency name (e.g. "nginx", "nginx/*", "*").
    pub match_pattern: String,
    /// Which update types this policy applies to. Empty means all types.
    #[serde(default)]
    pub update_types: Vec<UpdateTypeFilter>,
    /// Whether automerge is enabled for matches.
    #[serde(default = "default_policy_enabled")]
    pub enabled: bool,
    /// Minimum age in days before the MR may be automerged.
    #[serde(default)]
    pub minimum_age_days: Option<u32>,
}

fn default_policy_enabled() -> bool {
    true
}

/// Days of the week (ISO weekday numbering: Monday=1 … Sunday=7).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// Return the ISO weekday number (1 = Monday … 7 = Sunday).
    pub fn iso_number(&self) -> u32 {
        match self {
            Weekday::Monday => 1,
            Weekday::Tuesday => 2,
            Weekday::Wednesday => 3,
            Weekday::Thursday => 4,
            Weekday::Friday => 5,
            Weekday::Saturday => 6,
            Weekday::Sunday => 7,
        }
    }
}

/// Restricts when new MRs may be created.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleWindow {
    /// Days on which MR creation is allowed. Empty means all days.
    #[serde(default)]
    pub days: Vec<Weekday>,
    /// Hour (UTC, 0-23) at which the window opens (inclusive). None = start of day.
    #[serde(default)]
    pub hours_start: Option<u32>,
    /// Hour (UTC, 0-23) at which the window closes (exclusive). None = end of day.
    #[serde(default)]
    pub hours_end: Option<u32>,
}

/// Determines how candidates within a rule are grouped together.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    /// Group by semver bump type (patch/minor/major).
    UpdateType,
    /// Group by package manager (docker/helm).
    Manager,
    /// All matching candidates go into one group named after the rule.
    Pattern,
    /// Group by the directory path of the file containing the dependency.
    Path,
}

/// A rule that matches a set of dependencies and groups them into a single MR.
#[derive(Debug, Clone, Deserialize)]
pub struct GroupingRule {
    /// Name for this rule; used in branch names and MR titles.
    pub name: String,
    /// Glob patterns matched against the dependency name. Empty means match all.
    #[serde(default)]
    pub match_patterns: Vec<String>,
    /// How to sub-group the matched candidates.
    #[serde(default = "default_group_by")]
    pub group_by: GroupBy,
    /// When true, major version bumps are split out into their own separate group.
    #[serde(default)]
    pub separate_major: bool,
}

fn default_group_by() -> GroupBy {
    GroupBy::Pattern
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
    /// Fine-grained per-dependency automerge policies (evaluated in order, first match wins).
    #[serde(default)]
    pub automerge_policies: Vec<AutomergePolicy>,
    /// Maximum number of open reforge MRs allowed at any time. None = unlimited.
    #[serde(default)]
    pub max_open_mrs: Option<usize>,
    /// Time window during which new MRs may be created. None = always allowed.
    #[serde(default)]
    pub schedule_window: Option<ScheduleWindow>,
    /// Named grouping rules; matched candidates are combined into a single MR per group.
    #[serde(default)]
    pub grouping_rules: Vec<GroupingRule>,
}

impl Default for MergeRequestConfig {
    fn default() -> Self {
        Self {
            branch_prefix: default_branch_prefix(),
            labels: vec!["reforge".to_string(), "automated".to_string()],
            grouping: default_grouping(),
            assignees: vec![],
            auto_merge: false,
            automerge_policies: vec![],
            max_open_mrs: None,
            schedule_window: None,
            grouping_rules: vec![],
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
pub struct ChangelogConfig {
    /// Whether to fetch and embed changelog / release notes in MR descriptions.
    #[serde(default = "default_changelog_enabled")]
    pub enabled: bool,
    /// Maximum number of characters to include before truncating.
    #[serde(default = "default_changelog_max_length")]
    pub max_length: usize,
    /// GitHub personal access token for the releases API (loaded from GITHUB_TOKEN env).
    #[serde(skip)]
    pub github_token: Option<String>,
}

fn default_changelog_enabled() -> bool {
    true
}

fn default_changelog_max_length() -> usize {
    2000
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            enabled: default_changelog_enabled(),
            max_length: default_changelog_max_length(),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
        }
    }
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

        // Populate changelog token from env (field is skipped during deserialization).
        if config.changelog.github_token.is_none() {
            config.changelog.github_token = std::env::var("GITHUB_TOKEN").ok();
        }

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

        for rm in &config.regex_managers {
            rm.validate()?;
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
            changelog: ChangelogConfig::default(),
            local_path,
            regex_managers: vec![],
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
