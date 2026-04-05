use regex::{Regex, RegexBuilder};

use crate::config::{Datasource, RegexManagerConfig};
use crate::error::{ReforgeError, Result};
use crate::manager::{Dependency, PackageManager, RegistrySource, UpdateContext};

#[derive(Debug)]
pub struct RegexManager {
    config: RegexManagerConfig,
    /// Pre-compiled regex (multiline mode enabled).
    pattern: Regex,
}

impl RegexManager {
    pub fn new(config: RegexManagerConfig) -> Result<Self> {
        // Validate first (checks required groups and datasource).
        config.validate()?;

        // Compile with multi-line dot-all so patterns can span lines.
        let pattern = RegexBuilder::new(&config.match_pattern)
            .multi_line(true)
            .dot_matches_new_line(true)
            .build()
            .map_err(|e| {
                ReforgeError::Config(format!(
                    "regex_manager '{}': failed to compile pattern: {}",
                    config.name, e
                ))
            })?;

        Ok(Self { config, pattern })
    }

    fn map_datasource(
        &self,
        dep_name: &str,
        registry_url: Option<&str>,
    ) -> RegistrySource {
        let effective_registry = registry_url
            .map(|u| u.trim_end_matches('/').to_string())
            .or_else(|| self.config.registry_url.clone());

        match self.config.datasource {
            Datasource::Docker => {
                let registry = effective_registry.clone();
                let image = match &registry {
                    Some(reg) => format!("{}/{}", reg, dep_name),
                    None => dep_name.to_string(),
                };
                RegistrySource::DockerRegistry { image, registry }
            }
            Datasource::HelmOci => {
                let image = match &effective_registry {
                    Some(reg) => {
                        let reg = reg.trim_start_matches("oci://");
                        format!("{}/{}", reg, dep_name)
                    }
                    None => dep_name.to_string(),
                };
                let registry = image
                    .find('/')
                    .map(|idx| image[..idx].to_string());
                RegistrySource::OciHelmRegistry { image, registry }
            }
            Datasource::HelmRepo => {
                let repo_url = effective_registry
                    .unwrap_or_else(|| "https://charts.helm.sh/stable".to_string());
                RegistrySource::HelmRepository {
                    repo_url,
                    chart_name: dep_name.to_string(),
                }
            }
        }
    }
}

impl PackageManager for RegexManager {
    fn name(&self) -> &'static str {
        // We cannot return a reference into self.config.name here because the
        // trait returns `&'static str`. Use a fixed label; callers that need
        // the custom name can use the config directly.
        "regex"
    }

    fn file_patterns(&self) -> Vec<&'static str> {
        // The dynamic patterns are stored in self.config.file_patterns which
        // has non-'static lifetimes. The orchestrator handles regex managers
        // separately with dynamic pattern matching, so we return empty here.
        vec![]
    }

    fn extract_dependencies(&self, file_path: &str, contents: &str) -> Result<Vec<Dependency>> {
        let mut deps = Vec::new();

        for caps in self.pattern.captures_iter(contents) {
            let dep_name = match caps.name("depName") {
                Some(m) => m.as_str().to_string(),
                None => continue,
            };
            let current_value = match caps.name("currentValue") {
                Some(m) => m.as_str().to_string(),
                None => continue,
            };

            // Optional overrides captured from the file itself.
            let registry_url = caps.name("registryUrl").map(|m| m.as_str());

            // The full matched text is used by the updater to locate the span.
            let matched_text = caps[0].to_string();

            let registry = self.map_datasource(&dep_name, registry_url);

            deps.push(Dependency {
                name: dep_name,
                current_version: current_value.clone(),
                registry,
                file_path: file_path.to_string(),
                update_context: UpdateContext::RegexMatch {
                    matched_text,
                    old_value: current_value,
                },
            });
        }

        Ok(deps)
    }
}

/// Check whether a file path matches a dynamic glob-style pattern.
///
/// Supports leading `**/` (any directory prefix), `*` wildcards, and exact
/// filename matches.
pub fn file_matches_pattern(path: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_start_matches("**/");

    if pattern.contains('/') {
        // Pattern contains a slash: match against the full path.
        crate::util::glob_match(pattern, path)
    } else {
        // Pattern is a plain filename glob: match only against the filename.
        let filename = path.rsplit('/').next().unwrap_or(path);
        crate::util::glob_match(pattern, filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Datasource, RegexManagerConfig};

    fn make_config(
        datasource: Datasource,
        pattern: &str,
        registry_url: Option<&str>,
    ) -> RegexManagerConfig {
        RegexManagerConfig {
            name: "test".to_string(),
            file_patterns: vec!["*.yaml".to_string()],
            match_pattern: pattern.to_string(),
            datasource,
            registry_url: registry_url.map(|s| s.to_string()),
            versioning: None,
        }
    }

    #[test]
    fn test_helm_version_pattern() {
        // Match helmChart OCI URL (capturing last path segment as depName)
        // followed by helmVersion on the next line.
        let pattern =
            "helmChart:\\s*['\"]?(?:oci://[^'\"\\s]*/)?(?P<depName>[^'\"\\s/]+)['\"]?\\s*\nhelmVersion:\\s*['\"]?(?P<currentValue>[^'\"\\s]+)";
        let config = make_config(Datasource::HelmOci, pattern, Some("oci://oci-charts.example.com"));
        let mgr = RegexManager::new(config).unwrap();

        let contents = "\
appName: login\n\
helmChart: 'oci://oci-charts.example.com/developer-excellence/stateless-http-service'\n\
helmVersion: '14.1.0'\n";

        let deps = mgr.extract_dependencies("app.yaml", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "stateless-http-service");
        assert_eq!(deps[0].current_version, "14.1.0");
    }

    #[test]
    fn test_docker_datasource() {
        let pattern = r"image:\s*(?P<depName>[^:\s]+):(?P<currentValue>[^\s]+)";
        let config = make_config(Datasource::Docker, pattern, None);
        let mgr = RegexManager::new(config).unwrap();

        let contents = "image: nginx:1.25.3\n";
        let deps = mgr.extract_dependencies("deploy.yaml", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "nginx");
        assert_eq!(deps[0].current_version, "1.25.3");
        assert!(matches!(
            deps[0].registry,
            RegistrySource::DockerRegistry { .. }
        ));
    }

    #[test]
    fn test_helm_repo_datasource() {
        let pattern = r"chart:\s*(?P<depName>[^\s]+)\s*\nversion:\s*(?P<currentValue>[^\s]+)";
        let config = make_config(
            Datasource::HelmRepo,
            pattern,
            Some("https://charts.bitnami.com/bitnami"),
        );
        let mgr = RegexManager::new(config).unwrap();

        let contents = "chart: redis\nversion: 18.4.0\n";
        let deps = mgr.extract_dependencies("config.yaml", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "redis");
        assert_eq!(deps[0].current_version, "18.4.0");
        assert!(matches!(
            deps[0].registry,
            RegistrySource::HelmRepository { .. }
        ));
    }

    #[test]
    fn test_validation_missing_dep_name() {
        let config = make_config(
            Datasource::Docker,
            r"image:\s*(?P<currentValue>[^\s]+)",
            None,
        );
        let err = RegexManager::new(config).unwrap_err();
        assert!(err.to_string().contains("depName"));
    }

    #[test]
    fn test_validation_missing_current_value() {
        let config = make_config(
            Datasource::Docker,
            r"image:\s*(?P<depName>[^\s]+)",
            None,
        );
        let err = RegexManager::new(config).unwrap_err();
        assert!(err.to_string().contains("currentValue"));
    }

    #[test]
    fn test_validation_invalid_regex() {
        let config = make_config(Datasource::Docker, r"(?P<depName>[", None);
        let err = RegexManager::new(config).unwrap_err();
        assert!(err.to_string().contains("invalid") || err.to_string().contains("failed"));
    }

    #[test]
    fn test_multiple_matches() {
        let pattern = r"image:\s*(?P<depName>[^:\s]+):(?P<currentValue>[^\s]+)";
        let config = make_config(Datasource::Docker, pattern, None);
        let mgr = RegexManager::new(config).unwrap();

        let contents = "image: nginx:1.25.3\nimage: redis:7.2\n";
        let deps = mgr.extract_dependencies("deploy.yaml", contents).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "nginx");
        assert_eq!(deps[1].name, "redis");
    }

    #[test]
    fn test_registry_url_capture_group() {
        let pattern = r"(?P<registryUrl>https://[^/]+)/(?P<depName>[^:\s]+):(?P<currentValue>[^\s]+)";
        let config = make_config(Datasource::Docker, pattern, None);
        let mgr = RegexManager::new(config).unwrap();

        let contents = "https://registry.example.com/myapp:v1.2.3\n";
        let deps = mgr.extract_dependencies("config.yaml", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "myapp");
        assert_eq!(deps[0].current_version, "v1.2.3");
    }

    #[test]
    fn test_file_matches_pattern() {
        assert!(file_matches_pattern("apps/login/app.yaml", "*.yaml"));
        assert!(file_matches_pattern("Chart.yaml", "Chart.yaml"));
        assert!(!file_matches_pattern("Chart.json", "Chart.yaml"));
        assert!(file_matches_pattern("values-prod.yaml", "values-*.yaml"));
        assert!(!file_matches_pattern("values-prod.yaml", "Chart.yaml"));
        assert!(!file_matches_pattern("apps/login/app.yaml", "**/apps/*.yaml"));
        assert!(file_matches_pattern("apps/app.yaml", "**/apps/*.yaml"));
        assert!(file_matches_pattern("apps/login/app.yaml", "apps/**/*.yaml"));
    }
}
