use serde::Deserialize;
use tracing::warn;

use crate::error::{ReforgeError, Result};
use crate::manager::{Dependency, PackageManager, RegistrySource, UpdateContext};

pub struct HelmManager;

#[derive(Debug, Deserialize)]
struct ChartYaml {
    #[serde(default)]
    dependencies: Option<Vec<ChartDependency>>,
}

#[derive(Debug, Deserialize)]
struct ChartDependency {
    name: String,
    version: String,
    repository: String,
}

impl HelmManager {
    pub fn new() -> Self {
        Self
    }

    fn extract_chart_yaml_deps(
        &self,
        file_path: &str,
        contents: &str,
    ) -> Result<Vec<Dependency>> {
        let chart: ChartYaml = serde_yaml::from_str(contents).map_err(|e| {
            ReforgeError::Parse {
                file: file_path.to_string(),
                reason: format!("Chart.yaml parse error: {}", e),
            }
        })?;

        let mut deps = Vec::new();

        if let Some(dependencies) = chart.dependencies {
            for (idx, dep) in dependencies.iter().enumerate() {
                let registry = if dep.repository.starts_with("oci://") {
                    let oci_ref = dep.repository.trim_start_matches("oci://");
                    let full_image = format!("{}/{}", oci_ref, dep.name);
                    let (registry, _) = parse_oci_reference(&full_image);
                    RegistrySource::OciHelmRegistry {
                        image: full_image,
                        registry,
                    }
                } else if dep.repository.starts_with("http://")
                    || dep.repository.starts_with("https://")
                {
                    RegistrySource::HelmRepository {
                        repo_url: dep.repository.clone(),
                        chart_name: dep.name.clone(),
                    }
                } else if dep.repository.starts_with("alias:")
                    || dep.repository.starts_with('@')
                {
                    warn!(
                        "Skipping alias-based repository for {}: {}",
                        dep.name, dep.repository
                    );
                    continue;
                } else {
                    warn!(
                        "Unknown repository format for {}: {}",
                        dep.name, dep.repository
                    );
                    continue;
                };

                deps.push(Dependency {
                    name: dep.name.clone(),
                    current_version: dep.version.clone(),
                    registry,
                    file_path: file_path.to_string(),
                    update_context: UpdateContext::YamlKeyPath {
                        keys: vec![
                            "dependencies".to_string(),
                            idx.to_string(),
                            "version".to_string(),
                        ],
                    },
                });
            }
        }

        Ok(deps)
    }

    fn extract_values_yaml_deps(
        &self,
        file_path: &str,
        contents: &str,
    ) -> Result<Vec<Dependency>> {
        let yaml: serde_yaml::Value = serde_yaml::from_str(contents).map_err(|e| {
            ReforgeError::Parse {
                file: file_path.to_string(),
                reason: format!("values.yaml parse error: {}", e),
            }
        })?;

        let mut deps = Vec::new();
        self.walk_yaml_for_images(&yaml, &mut Vec::new(), file_path, &mut deps);
        Ok(deps)
    }

    fn walk_yaml_for_images(
        &self,
        value: &serde_yaml::Value,
        path: &mut Vec<String>,
        file_path: &str,
        deps: &mut Vec<Dependency>,
    ) {
        match value {
            serde_yaml::Value::Mapping(map) => {
                // Check if this mapping has repository+tag or image+tag siblings
                let repo_val = map
                    .get(serde_yaml::Value::String("repository".to_string()))
                    .or_else(|| map.get(serde_yaml::Value::String("image".to_string())));
                let tag_val = map
                    .get(serde_yaml::Value::String("tag".to_string()))
                    .or_else(|| map.get(serde_yaml::Value::String("version".to_string())));

                if let (Some(repo), Some(tag)) = (repo_val, tag_val) {
                    if let (Some(repo_str), Some(tag_str)) =
                        (repo.as_str(), tag_as_string(tag))
                    {
                        let (registry, image_name) = parse_docker_image(repo_str);
                        let mut key_path = path.clone();
                        // Determine which key name has the tag
                        let tag_key = if map
                            .get(serde_yaml::Value::String("tag".to_string()))
                            .is_some()
                        {
                            "tag"
                        } else {
                            "version"
                        };
                        key_path.push(tag_key.to_string());

                        deps.push(Dependency {
                            name: image_name.clone(),
                            current_version: tag_str,
                            registry: RegistrySource::DockerRegistry {
                                image: repo_str.to_string(),
                                registry,
                            },
                            file_path: file_path.to_string(),
                            update_context: UpdateContext::YamlKeyPath { keys: key_path },
                        });
                    }
                }

                // Recurse into child mappings
                for (key, val) in map {
                    if let Some(key_str) = key.as_str() {
                        path.push(key_str.to_string());
                        self.walk_yaml_for_images(val, path, file_path, deps);
                        path.pop();
                    }
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                for (idx, val) in seq.iter().enumerate() {
                    path.push(idx.to_string());
                    self.walk_yaml_for_images(val, path, file_path, deps);
                    path.pop();
                }
            }
            _ => {}
        }
    }
}

fn tag_as_string(val: &serde_yaml::Value) -> Option<String> {
    match val {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_docker_image(image: &str) -> (Option<String>, String) {
    if let Some(idx) = image.rfind('/') {
        let prefix = &image[..idx];
        let name = &image[idx + 1..];
        if prefix.contains('.') || prefix.contains(':') {
            return (Some(prefix.to_string()), name.to_string());
        }
        (None, image.to_string())
    } else {
        (None, image.to_string())
    }
}

fn parse_oci_reference(image: &str) -> (Option<String>, String) {
    if let Some(idx) = image.find('/') {
        let registry = &image[..idx];
        (Some(registry.to_string()), image.to_string())
    } else {
        (None, image.to_string())
    }
}

impl PackageManager for HelmManager {
    fn name(&self) -> &'static str {
        "helm"
    }

    fn file_patterns(&self) -> Vec<&'static str> {
        vec!["Chart.yaml", "values.yaml", "values-*.yaml"]
    }

    fn extract_dependencies(&self, file_path: &str, contents: &str) -> Result<Vec<Dependency>> {
        let filename = file_path.rsplit('/').next().unwrap_or(file_path);

        if filename == "Chart.yaml" {
            self.extract_chart_yaml_deps(file_path, contents)
        } else {
            self.extract_values_yaml_deps(file_path, contents)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chart_yaml_http_repo() {
        let mgr = HelmManager::new();
        let contents = r#"
apiVersion: v2
name: my-app
version: 1.0.0
dependencies:
  - name: ingress-nginx
    version: 4.8.3
    repository: https://kubernetes.github.io/ingress-nginx
  - name: redis
    version: 18.4.0
    repository: https://charts.bitnami.com/bitnami
"#;
        let deps = mgr.extract_dependencies("Chart.yaml", contents).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "ingress-nginx");
        assert_eq!(deps[0].current_version, "4.8.3");
        assert_eq!(deps[1].name, "redis");
        assert_eq!(deps[1].current_version, "18.4.0");
    }

    #[test]
    fn test_chart_yaml_oci_repo() {
        let mgr = HelmManager::new();
        let contents = r#"
apiVersion: v2
name: my-app
version: 1.0.0
dependencies:
  - name: my-chart
    version: 2.0.0
    repository: oci://registry.example.com/charts
"#;
        let deps = mgr.extract_dependencies("Chart.yaml", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "my-chart");
        assert_eq!(deps[0].current_version, "2.0.0");
        match &deps[0].registry {
            RegistrySource::OciHelmRegistry { image, .. } => {
                assert_eq!(image, "registry.example.com/charts/my-chart");
            }
            _ => panic!("Expected OciHelmRegistry"),
        }
    }

    #[test]
    fn test_chart_yaml_alias_skipped() {
        let mgr = HelmManager::new();
        let contents = r#"
apiVersion: v2
name: my-app
version: 1.0.0
dependencies:
  - name: local-dep
    version: 1.0.0
    repository: alias:my-alias
"#;
        let deps = mgr.extract_dependencies("Chart.yaml", contents).unwrap();
        assert_eq!(deps.len(), 0);
    }

    #[test]
    fn test_values_yaml_image_tag() {
        let mgr = HelmManager::new();
        let contents = r#"
image:
  repository: nginx
  tag: "1.25.3"
  pullPolicy: IfNotPresent
"#;
        let deps = mgr.extract_dependencies("values.yaml", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "nginx");
        assert_eq!(deps[0].current_version, "1.25.3");
    }

    #[test]
    fn test_values_yaml_nested_images() {
        let mgr = HelmManager::new();
        let contents = r#"
app:
  frontend:
    image:
      repository: myorg/frontend
      tag: "2.1.0"
  backend:
    image:
      repository: myorg/backend
      tag: "3.0.1"
"#;
        let deps = mgr.extract_dependencies("values.yaml", contents).unwrap();
        assert_eq!(deps.len(), 2);
    }

    #[test]
    fn test_values_yaml_numeric_tag() {
        let mgr = HelmManager::new();
        let contents = r#"
image:
  repository: redis
  tag: 7.2
"#;
        let deps = mgr.extract_dependencies("values.yaml", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].current_version, "7.2");
    }
}
