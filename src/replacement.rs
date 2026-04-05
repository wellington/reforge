use serde::Deserialize;

use crate::error::{ReforgeError, Result};
use crate::manager::Dependency;

/// A rule describing that one image/chart name should be replaced by another,
/// or that a name is deprecated with no known replacement.
#[derive(Debug, Clone, Deserialize)]
pub struct ReplacementRule {
    /// The old image/chart name (may contain a `*` glob wildcard).
    pub old_name: String,
    /// The new image/chart name. May use `*` to carry forward a captured suffix
    /// when `old_name` ends with a wildcard.
    pub new_name: String,
    /// Registry that the old name lives in (e.g. `gcr.io/google-containers`).
    #[serde(default)]
    pub old_registry: Option<String>,
    /// Registry the new name lives in.
    #[serde(default)]
    pub new_registry: Option<String>,
    /// Human-readable description of why the migration is needed.
    #[serde(default)]
    pub reason: Option<String>,
    /// When `true` the old name is deprecated without a working replacement.
    #[serde(default)]
    pub deprecated_only: bool,
}

impl ReplacementRule {
    /// Returns `true` when `name` (and optionally `registry`) match this rule.
    /// Supports a trailing-`*` glob on `old_name`.
    pub fn matches(&self, dep_name: &str, registry: Option<&str>) -> bool {
        if let Some(rule_registry) = &self.old_registry {
            match registry {
                Some(r) if r == rule_registry => {}
                _ => return false,
            }
        }

        glob_match(&self.old_name, dep_name)
    }

    /// Given a matched `dep_name`, resolve the concrete `new_name` (expanding
    /// any wildcard suffix).
    pub fn resolve_new_name(&self, dep_name: &str) -> String {
        if self.old_name.ends_with('*') && self.new_name.ends_with('*') {
            let prefix_len = self.old_name.len() - 1;
            let suffix = &dep_name[prefix_len.min(dep_name.len())..];
            let new_prefix = &self.new_name[..self.new_name.len() - 1];
            format!("{}{}", new_prefix, suffix)
        } else {
            self.new_name.clone()
        }
    }
}

fn glob_match(pattern: &str, value: &str) -> bool {
    crate::util::glob_match(pattern, value)
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ReplacementDatabase {
    pub rules: Vec<ReplacementRule>,
}

impl ReplacementDatabase {
    /// Load rules from a TOML file.
    ///
    /// Expected format:
    /// ```toml
    /// [[rules]]
    /// old_name = "foo"
    /// new_name = "bar"
    /// reason   = "foo was renamed to bar"
    /// ```
    pub fn load_from_toml(path: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct TomlFile {
            #[serde(default)]
            rules: Vec<ReplacementRule>,
        }

        let contents = std::fs::read_to_string(path).map_err(|e| {
            ReforgeError::Config(format!(
                "Failed to read replacements file '{}': {}",
                path, e
            ))
        })?;

        let file: TomlFile = toml::from_str(&contents).map_err(|e| {
            ReforgeError::Config(format!(
                "Invalid replacements file '{}': {}",
                path, e
            ))
        })?;

        Ok(Self { rules: file.rules })
    }

    /// Returns a database pre-populated with well-known replacements.
    pub fn load_builtin() -> Self {
        let rules = vec![
            // Google Containers → registry.k8s.io (whole namespace glob)
            ReplacementRule {
                old_name: "*".to_string(),
                new_name: "*".to_string(),
                old_registry: Some("gcr.io/google-containers".to_string()),
                new_registry: Some("registry.k8s.io".to_string()),
                reason: Some(
                    "gcr.io/google-containers is deprecated; images have moved to registry.k8s.io".to_string(),
                ),
                deprecated_only: false,
            },
            // nginx → unprivileged variant
            ReplacementRule {
                old_name: "nginx".to_string(),
                new_name: "nginxinc/nginx-unprivileged".to_string(),
                old_registry: Some("docker.io/library".to_string()),
                new_registry: Some("docker.io".to_string()),
                reason: Some(
                    "The official nginx image runs as root. nginxinc/nginx-unprivileged is the recommended rootless alternative.".to_string(),
                ),
                deprecated_only: false,
            },
            // quay.io/coreos/etcd — deprecated, no replacement via this DB
            ReplacementRule {
                old_name: "etcd".to_string(),
                new_name: "etcd".to_string(),
                old_registry: Some("quay.io/coreos".to_string()),
                new_registry: None,
                reason: Some(
                    "quay.io/coreos/etcd is no longer maintained. Use the etcd image from registry.k8s.io or your distribution.".to_string(),
                ),
                deprecated_only: true,
            },
        ];
        Self { rules }
    }

    /// Find the first rule that matches `dep_name` and `registry`.
    pub fn find_replacement<'a>(
        &'a self,
        dep_name: &str,
        registry: Option<&str>,
    ) -> Option<&'a ReplacementRule> {
        self.rules.iter().find(|r| r.matches(dep_name, registry))
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ReplacementAction {
    Replace {
        dep_name: String,
        file_path: String,
        from_ref: String,
        to_ref: String,
        reason: Option<String>,
    },
    DeprecationWarning {
        dep_name: String,
        file_path: String,
        reason: Option<String>,
    },
}

/// Check each dependency against the database and return the actions required.
pub fn check_dependencies(deps: &[Dependency], db: &ReplacementDatabase) -> Vec<ReplacementAction> {
    let mut actions = Vec::new();

    for dep in deps {
        let (dep_image, registry) = dep_name_and_registry(dep);

        if let Some(rule) = db.find_replacement(&dep_image, registry.as_deref()) {
            if rule.deprecated_only {
                actions.push(ReplacementAction::DeprecationWarning {
                    dep_name: dep.name.clone(),
                    file_path: dep.file_path.clone(),
                    reason: rule.reason.clone(),
                });
            } else {
                let new_name = rule.resolve_new_name(&dep_image);
                let new_reg = rule.new_registry.as_deref().unwrap_or("");
                let to_ref = if new_reg.is_empty() {
                    new_name.clone()
                } else {
                    format!("{}/{}", new_reg, new_name)
                };

                let from_ref = match &registry {
                    Some(r) => format!("{}/{}", r, dep_image),
                    None => dep_image.clone(),
                };

                actions.push(ReplacementAction::Replace {
                    dep_name: dep.name.clone(),
                    file_path: dep.file_path.clone(),
                    from_ref,
                    to_ref,
                    reason: rule.reason.clone(),
                });
            }
        }
    }

    actions
}

/// Extract the image/chart name and registry from a `Dependency`.
fn dep_name_and_registry(dep: &Dependency) -> (String, Option<String>) {
    use crate::manager::RegistrySource;

    match &dep.registry {
        RegistrySource::DockerRegistry { image, registry } => {
            (image.clone(), registry.clone())
        }
        RegistrySource::HelmRepository { chart_name, repo_url } => {
            (chart_name.clone(), Some(repo_url.clone()))
        }
        RegistrySource::OciHelmRegistry { image, registry } => {
            (image.clone(), registry.clone())
        }
    }
}

// ---------------------------------------------------------------------------
// MR body rendering
// ---------------------------------------------------------------------------

pub fn render_replacement_mr_body(action: &ReplacementAction) -> String {
    match action {
        ReplacementAction::Replace {
            dep_name,
            file_path,
            from_ref,
            to_ref,
            reason,
        } => {
            let reason_section = match reason {
                Some(r) => format!("\n\n**Reason:** {}", r),
                None => String::new(),
            };
            format!(
                "## Image/Chart Migration: `{}`\n\n\
                 This dependency has been renamed or relocated and requires migration.\n\n\
                 | | Reference |\n\
                 |---|---|\n\
                 | **Before** | `{}` |\n\
                 | **After**  | `{}` |\n\n\
                 **File:** `{}`{}\n\n\
                 ---\n\n\
                 *This MR was automatically created by reforge.*",
                dep_name, from_ref, to_ref, file_path, reason_section,
            )
        }
        ReplacementAction::DeprecationWarning {
            dep_name,
            file_path,
            reason,
        } => {
            let reason_section = match reason {
                Some(r) => format!("\n\n**Details:** {}", r),
                None => String::new(),
            };
            format!(
                "## Deprecation Notice: `{}`\n\n\
                 The image/chart `{}` used in `{}` is **deprecated**. \
                 No automated migration is available — please review manually.{}\n\n\
                 ---\n\n\
                 *This notice was generated by reforge.*",
                dep_name, dep_name, file_path, reason_section,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::{Dependency, RegistrySource, UpdateContext};

    fn docker_dep(name: &str, image: &str, registry: Option<&str>) -> Dependency {
        Dependency {
            name: name.to_string(),
            current_version: "1.0.0".to_string(),
            registry: RegistrySource::DockerRegistry {
                image: image.to_string(),
                registry: registry.map(|s| s.to_string()),
            },
            file_path: "Dockerfile".to_string(),
            update_context: UpdateContext::DockerFrom {
                line_number: 0,
                full_reference: format!("{}:1.0.0", image),
            },
        }
    }

    // --- glob_match ---

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("nginx", "nginx"));
        assert!(!glob_match("nginx", "nginx-unprivileged"));
    }

    #[test]
    fn test_glob_match_wildcard_prefix() {
        assert!(glob_match("gcr.io/google-containers/*", "gcr.io/google-containers/pause"));
        assert!(!glob_match("gcr.io/google-containers/*", "gcr.io/other/pause"));
    }

    #[test]
    fn test_glob_match_trailing_star() {
        assert!(glob_match("kube*", "kube-proxy"));
        assert!(glob_match("kube*", "kubernetes"));
        assert!(!glob_match("kube*", "nginx"));
    }

    // --- ReplacementRule::matches ---

    #[test]
    fn test_rule_matches_with_registry() {
        let rule = ReplacementRule {
            old_name: "nginx".to_string(),
            new_name: "nginxinc/nginx-unprivileged".to_string(),
            old_registry: Some("docker.io/library".to_string()),
            new_registry: Some("docker.io".to_string()),
            reason: None,
            deprecated_only: false,
        };
        assert!(rule.matches("nginx", Some("docker.io/library")));
        assert!(!rule.matches("nginx", Some("quay.io")));
        assert!(!rule.matches("nginx", None));
    }

    #[test]
    fn test_rule_matches_without_registry_constraint() {
        let rule = ReplacementRule {
            old_name: "old-image".to_string(),
            new_name: "new-image".to_string(),
            old_registry: None,
            new_registry: None,
            reason: None,
            deprecated_only: false,
        };
        assert!(rule.matches("old-image", None));
        assert!(rule.matches("old-image", Some("any.registry.io")));
        assert!(!rule.matches("other-image", None));
    }

    #[test]
    fn test_rule_resolve_new_name_glob() {
        let rule = ReplacementRule {
            old_name: "*".to_string(),
            new_name: "*".to_string(),
            old_registry: Some("gcr.io/google-containers".to_string()),
            new_registry: Some("registry.k8s.io".to_string()),
            reason: None,
            deprecated_only: false,
        };
        assert_eq!(rule.resolve_new_name("pause"), "pause");
        assert_eq!(rule.resolve_new_name("kube-apiserver"), "kube-apiserver");
    }

    #[test]
    fn test_rule_resolve_new_name_exact() {
        let rule = ReplacementRule {
            old_name: "nginx".to_string(),
            new_name: "nginxinc/nginx-unprivileged".to_string(),
            old_registry: None,
            new_registry: None,
            reason: None,
            deprecated_only: false,
        };
        assert_eq!(rule.resolve_new_name("nginx"), "nginxinc/nginx-unprivileged");
    }

    // --- ReplacementDatabase built-in ---

    #[test]
    fn test_builtin_database_has_gcr_rule() {
        let db = ReplacementDatabase::load_builtin();
        let rule = db.find_replacement("pause", Some("gcr.io/google-containers"));
        assert!(rule.is_some());
        let r = rule.unwrap();
        assert_eq!(r.new_registry.as_deref(), Some("registry.k8s.io"));
    }

    #[test]
    fn test_builtin_database_has_nginx_rule() {
        let db = ReplacementDatabase::load_builtin();
        let rule = db.find_replacement("nginx", Some("docker.io/library"));
        assert!(rule.is_some());
        assert!(!rule.unwrap().deprecated_only);
    }

    #[test]
    fn test_builtin_database_has_etcd_deprecation() {
        let db = ReplacementDatabase::load_builtin();
        let rule = db.find_replacement("etcd", Some("quay.io/coreos"));
        assert!(rule.is_some());
        assert!(rule.unwrap().deprecated_only);
    }

    #[test]
    fn test_builtin_database_no_match() {
        let db = ReplacementDatabase::load_builtin();
        assert!(db.find_replacement("completely-unknown", None).is_none());
    }

    // --- check_dependencies ---

    #[test]
    fn test_check_dependencies_replace() {
        let db = ReplacementDatabase::load_builtin();
        let dep = docker_dep("nginx", "nginx", Some("docker.io/library"));
        let actions = check_dependencies(&[dep], &db);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ReplacementAction::Replace { from_ref, to_ref, .. } => {
                assert!(from_ref.contains("nginx"));
                assert!(to_ref.contains("nginx-unprivileged"));
            }
            other => panic!("expected Replace, got {:?}", other),
        }
    }

    #[test]
    fn test_check_dependencies_deprecation() {
        let db = ReplacementDatabase::load_builtin();
        let dep = docker_dep("etcd", "etcd", Some("quay.io/coreos"));
        let actions = check_dependencies(&[dep], &db);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ReplacementAction::DeprecationWarning { dep_name, .. } => {
                assert_eq!(dep_name, "etcd");
            }
            other => panic!("expected DeprecationWarning, got {:?}", other),
        }
    }

    #[test]
    fn test_check_dependencies_no_match() {
        let db = ReplacementDatabase::load_builtin();
        let dep = docker_dep("postgres", "postgres", Some("docker.io/library"));
        let actions = check_dependencies(&[dep], &db);
        assert!(actions.is_empty());
    }

    // --- render_replacement_mr_body ---

    #[test]
    fn test_render_replace_action() {
        let action = ReplacementAction::Replace {
            dep_name: "nginx".to_string(),
            file_path: "Dockerfile".to_string(),
            from_ref: "docker.io/library/nginx".to_string(),
            to_ref: "docker.io/nginxinc/nginx-unprivileged".to_string(),
            reason: Some("Use the rootless variant".to_string()),
        };
        let body = render_replacement_mr_body(&action);
        assert!(body.contains("Migration"));
        assert!(body.contains("docker.io/library/nginx"));
        assert!(body.contains("nginx-unprivileged"));
        assert!(body.contains("rootless"));
    }

    #[test]
    fn test_render_deprecation_action() {
        let action = ReplacementAction::DeprecationWarning {
            dep_name: "etcd".to_string(),
            file_path: "Chart.yaml".to_string(),
            reason: Some("No longer maintained".to_string()),
        };
        let body = render_replacement_mr_body(&action);
        assert!(body.contains("Deprecation"));
        assert!(body.contains("etcd"));
        assert!(body.contains("No longer maintained"));
        assert!(body.contains("review manually"));
    }

    #[test]
    fn test_render_replace_no_reason() {
        let action = ReplacementAction::Replace {
            dep_name: "old-img".to_string(),
            file_path: "deploy.yaml".to_string(),
            from_ref: "old-img".to_string(),
            to_ref: "new-img".to_string(),
            reason: None,
        };
        let body = render_replacement_mr_body(&action);
        assert!(body.contains("old-img"));
        assert!(body.contains("new-img"));
        assert!(!body.contains("**Reason:**"));
    }

    // --- load_from_toml ---

    #[test]
    fn test_load_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("replacements.toml");
        std::fs::write(
            &path,
            r#"
[[rules]]
old_name = "old-image"
new_name = "new-image"
reason = "renamed"
"#,
        )
        .unwrap();

        let db = ReplacementDatabase::load_from_toml(path.to_str().unwrap()).unwrap();
        assert_eq!(db.rules.len(), 1);
        assert_eq!(db.rules[0].old_name, "old-image");
        assert_eq!(db.rules[0].new_name, "new-image");
    }

    #[test]
    fn test_load_from_toml_missing_file() {
        let result = ReplacementDatabase::load_from_toml("/nonexistent/path/replacements.toml");
        assert!(result.is_err());
    }
}
