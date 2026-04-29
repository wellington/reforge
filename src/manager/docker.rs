//! Docker/OCI container image dependency manager.
//!
//! Extracts image references from Dockerfiles and docker-compose files.

use regex::Regex;
use std::sync::LazyLock;

use crate::error::{ReforgeError, Result};
use crate::manager::{Dependency, PackageManager, RegistrySource, UpdateContext};

/// Extracts Docker image dependencies from Dockerfiles and compose files.
#[derive(Debug, Default)]
pub struct DockerManager;

/// Matches `FROM` instructions, capturing the image reference.
static FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^FROM\s+(?:--platform=\S+\s+)?(?P<reference>[^\s]+)\s*(?:AS\s+\S+)?$"
    ).expect("FROM_RE is a valid regex")
});

/// Matches `ARG` instructions that define image references with tags.
static ARG_IMAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ARG\s+(\w+)=(.+?)(?::(?P<tag>[^\s@]+))?(?:@sha256:\w+)?\s*$")
        .expect("ARG_IMAGE_RE is a valid regex")
});

/// Matches `FROM` instructions that use variable substitution.
static FROM_VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^FROM\s+(?:--platform=\S+\s+)?\$\{?(\w+)\}?\s*")
        .expect("FROM_VAR_RE is a valid regex")
});

impl DockerManager {
    pub fn new() -> Self {
        Self
    }

    fn parse_image_reference(image: &str) -> (Option<String>, String) {
        crate::util::parse_image_reference(image)
    }

    fn extract_dockerfile_deps(
        &self,
        file_path: &str,
        contents: &str,
    ) -> Result<Vec<Dependency>> {
        let mut deps = Vec::new();
        let mut arg_vars: std::collections::HashMap<String, (String, String, usize)> =
            std::collections::HashMap::new();
        let mut from_var_refs: Vec<(String, usize)> = Vec::new();

        for (line_num, line) in contents.lines().enumerate() {
            let trimmed = line.trim();

            if let Some(caps) = ARG_IMAGE_RE.captures(trimmed) {
                let var_name = caps[1].to_string();
                let image_part = caps[2].to_string();
                if let Some(tag) = caps.name("tag") {
                    arg_vars.insert(
                        var_name,
                        (image_part, tag.as_str().to_string(), line_num),
                    );
                }
                continue;
            }

            if let Some(caps) = FROM_VAR_RE.captures(trimmed) {
                from_var_refs.push((caps[1].to_string(), line_num));
                continue;
            }

            if let Some(caps) = FROM_RE.captures(trimmed) {
                let reference = caps.name("reference").unwrap().as_str();

                // Skip digest-only references like nginx@sha256:abc123
                if reference.contains('@') && !reference.contains(':') {
                    continue;
                }

                let (image, tag) = match Self::split_image_tag(reference) {
                    Some(pair) => pair,
                    None => continue, // no tag (e.g. FROM scratch)
                };

                let (registry, image_name) = Self::parse_image_reference(&image);

                deps.push(Dependency {
                    name: image_name.clone(),
                    current_version: tag.clone(),
                    registry: RegistrySource::DockerRegistry {
                        image: image_name,
                        registry,
                    },
                    file_path: file_path.to_string(),
                    update_context: UpdateContext::DockerFrom {
                        line_number: line_num,
                        full_reference: format!("{}:{}", image, tag),
                    },
                });
            }
        }

        // Resolve ARG-based FROM references
        for (var_name, _from_line) in &from_var_refs {
            if let Some((image, tag, arg_line)) = arg_vars.get(var_name) {
                let (registry, image_name) = Self::parse_image_reference(image);
                let full_ref = format!("{}:{}", image, tag);

                deps.push(Dependency {
                    name: image_name.clone(),
                    current_version: tag.clone(),
                    registry: RegistrySource::DockerRegistry {
                        image: image_name,
                        registry,
                    },
                    file_path: file_path.to_string(),
                    update_context: UpdateContext::DockerFrom {
                        line_number: *arg_line,
                        full_reference: full_ref,
                    },
                });
            }
        }

        Ok(deps)
    }

    fn extract_compose_deps(
        &self,
        file_path: &str,
        contents: &str,
    ) -> Result<Vec<Dependency>> {
        let yaml: serde_yaml::Value = serde_yaml::from_str(contents).map_err(|e| {
            ReforgeError::Parse {
                file: file_path.to_string(),
                reason: format!("YAML parse error: {}", e),
            }
        })?;

        let mut deps = Vec::new();

        let services = match yaml.get("services") {
            Some(serde_yaml::Value::Mapping(m)) => m,
            _ => return Ok(deps),
        };

        for (svc_name, svc_config) in services {
            let svc_name_str = svc_name.as_str().unwrap_or("unknown");

            if let Some(image_val) = svc_config.get("image") {
                if let Some(image_str) = image_val.as_str() {
                    if let Some((image, tag)) = Self::split_image_tag(image_str) {
                        let (registry, image_name) = Self::parse_image_reference(&image);
                        deps.push(Dependency {
                            name: image_name.clone(),
                            current_version: tag.clone(),
                            registry: RegistrySource::DockerRegistry {
                                image: image_name,
                                registry,
                            },
                            file_path: file_path.to_string(),
                            update_context: UpdateContext::DockerComposeImage {
                                service_path: vec![
                                    "services".to_string(),
                                    svc_name_str.to_string(),
                                    "image".to_string(),
                                ],
                                full_reference: image_str.to_string(),
                            },
                        });
                    }
                }
            }
        }

        Ok(deps)
    }

    fn split_image_tag(image_str: &str) -> Option<(String, String)> {
        // Handle digest format
        let image_str = if let Some(at_idx) = image_str.find('@') {
            &image_str[..at_idx]
        } else {
            image_str
        };

        // Find the tag separator - last colon that's not part of a port in the registry
        if let Some(colon_idx) = image_str.rfind(':') {
            let potential_tag = &image_str[colon_idx + 1..];
            // If the part after colon contains '/', it's a port not a tag
            if !potential_tag.contains('/') {
                let image = image_str[..colon_idx].to_string();
                let tag = potential_tag.to_string();
                return Some((image, tag));
            }
        }
        None
    }
}

impl PackageManager for DockerManager {
    fn name(&self) -> &'static str {
        "docker"
    }

    fn file_patterns(&self) -> Vec<&'static str> {
        vec![
            "Dockerfile",
            "Dockerfile.*",
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
        ]
    }

    fn extract_dependencies(&self, file_path: &str, contents: &str) -> Result<Vec<Dependency>> {
        let filename = file_path
            .rsplit('/')
            .next()
            .unwrap_or(file_path);

        if filename.starts_with("Dockerfile") {
            self.extract_dockerfile_deps(file_path, contents)
        } else {
            self.extract_compose_deps(file_path, contents)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_from() {
        let mgr = DockerManager::new();
        let contents = "FROM nginx:1.25.3\n";
        let deps = mgr.extract_dependencies("Dockerfile", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "nginx");
        assert_eq!(deps[0].current_version, "1.25.3");
    }

    #[test]
    fn test_from_with_as() {
        let mgr = DockerManager::new();
        let contents = "FROM nginx:1.25.3 AS builder\n";
        let deps = mgr.extract_dependencies("Dockerfile", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].current_version, "1.25.3");
    }

    #[test]
    fn test_from_with_platform() {
        let mgr = DockerManager::new();
        let contents = "FROM --platform=linux/amd64 nginx:1.25.3\n";
        let deps = mgr.extract_dependencies("Dockerfile", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].current_version, "1.25.3");
    }

    #[test]
    fn test_from_with_registry() {
        let mgr = DockerManager::new();
        let contents = "FROM registry.example.com/myorg/myimage:v2.1.0\n";
        let deps = mgr.extract_dependencies("Dockerfile", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "myimage");
        assert_eq!(deps[0].current_version, "v2.1.0");
        match &deps[0].registry {
            RegistrySource::DockerRegistry { registry, .. } => {
                assert_eq!(registry.as_deref(), Some("registry.example.com/myorg"));
            }
            _ => panic!("Expected DockerRegistry"),
        }
    }

    #[test]
    fn test_multistage() {
        let mgr = DockerManager::new();
        let contents = "FROM golang:1.22 AS builder\nRUN go build\nFROM alpine:3.19\nCOPY --from=builder /app /app\n";
        let deps = mgr.extract_dependencies("Dockerfile", contents).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "golang");
        assert_eq!(deps[0].current_version, "1.22");
        assert_eq!(deps[1].name, "alpine");
        assert_eq!(deps[1].current_version, "3.19");
    }

    #[test]
    fn test_arg_based() {
        let mgr = DockerManager::new();
        let contents = "ARG BASE_IMAGE=nginx:1.25.3\nFROM ${BASE_IMAGE}\n";
        let deps = mgr.extract_dependencies("Dockerfile", contents).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "nginx");
        assert_eq!(deps[0].current_version, "1.25.3");
    }

    #[test]
    fn test_digest_only_skipped() {
        let mgr = DockerManager::new();
        let contents =
            "FROM nginx@sha256:abc123def456\n";
        let deps = mgr.extract_dependencies("Dockerfile", contents).unwrap();
        assert_eq!(deps.len(), 0);
    }

    #[test]
    fn test_compose_image() {
        let mgr = DockerManager::new();
        let contents = r#"
services:
  web:
    image: nginx:1.25.3
  redis:
    image: redis:7.2
"#;
        let deps = mgr
            .extract_dependencies("docker-compose.yaml", contents)
            .unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "nginx");
        assert_eq!(deps[0].current_version, "1.25.3");
        assert_eq!(deps[1].name, "redis");
        assert_eq!(deps[1].current_version, "7.2");
    }

    #[test]
    fn test_split_image_tag() {
        assert_eq!(
            DockerManager::split_image_tag("nginx:1.25"),
            Some(("nginx".into(), "1.25".into()))
        );
        assert_eq!(
            DockerManager::split_image_tag("registry.io:5000/img:v1"),
            Some(("registry.io:5000/img".into(), "v1".into()))
        );
        assert_eq!(DockerManager::split_image_tag("nginx"), None);
    }
}
