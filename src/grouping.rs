use std::collections::HashMap;

use crate::automerge::UpdateType;
use crate::config::{GroupBy, GroupingRule};
use crate::manager::RegistrySource;
use crate::orchestrator::UpdateCandidate;

/// A named collection of update candidates that should be combined into a single MR.
#[derive(Debug)]
pub struct Group {
    /// Human-readable name for this group (used in branch names and MR titles).
    pub name: String,
    /// The candidates to include in this group.
    pub candidates: Vec<UpdateCandidate>,
}

/// Partition `candidates` into groups according to the configured rules.
///
/// Matching rules are evaluated in order; the first matching rule wins for each
/// candidate. Unmatched candidates are handled according to `default_grouping`:
///   - `"grouped"` → all unmatched candidates land in a single "all" group.
///   - anything else (e.g. `"per-dependency"`) → each candidate gets its own group.
pub fn group_candidates(
    candidates: Vec<UpdateCandidate>,
    rules: &[GroupingRule],
    default_grouping: &str,
) -> Vec<Group> {
    let mut rule_buckets: HashMap<String, Vec<UpdateCandidate>> = HashMap::new();
    let mut unmatched: Vec<UpdateCandidate> = Vec::new();

    'outer: for candidate in candidates {
        for rule in rules {
            if rule_matches(rule, &candidate) {
                let sub_key = sub_group_key(rule, &candidate);
                let bucket_name = if sub_key.is_empty() {
                    rule.name.clone()
                } else {
                    format!("{}-{}", rule.name, sub_key)
                };
                rule_buckets
                    .entry(bucket_name)
                    .or_default()
                    .push(candidate);
                continue 'outer;
            }
        }
        unmatched.push(candidate);
    }

    // Preserve rule ordering for rule-based groups.
    let mut groups: Vec<Group> = Vec::new();

    // Build rule groups in the order the rules appear (then sub-keys sorted).
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rule in rules {
        // Collect all bucket names that stem from this rule.
        let mut matching_keys: Vec<String> = rule_buckets
            .keys()
            .filter(|k| k.starts_with(&rule.name))
            .cloned()
            .collect();
        matching_keys.sort();

        for key in matching_keys {
            if seen_names.contains(&key) {
                continue;
            }
            seen_names.insert(key.clone());
            if let Some(candidates) = rule_buckets.remove(&key) {
                groups.push(Group {
                    name: key,
                    candidates,
                });
            }
        }
    }

    // Handle unmatched candidates.
    match default_grouping {
        "grouped" => {
            if !unmatched.is_empty() {
                groups.push(Group {
                    name: "all".to_string(),
                    candidates: unmatched,
                });
            }
        }
        _ => {
            for candidate in unmatched {
                let manager = manager_name(&candidate.dependency.registry);
                let sanitized = candidate.dependency.name.replace('/', "-");
                groups.push(Group {
                    name: format!("{}-{}", manager, sanitized),
                    candidates: vec![candidate],
                });
            }
        }
    }

    groups
}

/// Returns true when the candidate should be processed by `rule`.
fn rule_matches(rule: &GroupingRule, candidate: &UpdateCandidate) -> bool {
    if rule.match_patterns.is_empty() {
        return true;
    }
    rule.match_patterns
        .iter()
        .any(|pat| glob_match(pat, &candidate.dependency.name))
}

/// Computes a sub-group discriminator key based on `GroupBy`.
///
/// When `separate_major` is set and the bump is major, we append "-major" to
/// whatever key would normally be produced.
fn sub_group_key(rule: &GroupingRule, candidate: &UpdateCandidate) -> String {
    let update_type = UpdateType::classify(
        &candidate.dependency.current_version,
        &candidate.new_version.original_tag,
    );

    let is_major = matches!(update_type, Some(UpdateType::Major));
    if rule.separate_major && is_major {
        return "major".to_string();
    }

    match rule.group_by {
        GroupBy::Pattern => String::new(),
        GroupBy::UpdateType => match update_type {
            Some(UpdateType::Patch) => "patch".to_string(),
            Some(UpdateType::Minor) => "minor".to_string(),
            Some(UpdateType::Major) => "major".to_string(),
            None => "unknown".to_string(),
        },
        GroupBy::Manager => manager_name(&candidate.dependency.registry).to_string(),
        GroupBy::Path => {
            let path = &candidate.dependency.file_path;
            // Use the parent directory of the file.
            std::path::Path::new(path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .replace('/', "-")
        }
    }
}

fn manager_name(registry: &RegistrySource) -> &'static str {
    match registry {
        RegistrySource::DockerRegistry { .. } => "docker",
        RegistrySource::HelmRepository { .. } => "helm",
        RegistrySource::OciHelmRegistry { .. } => "helm",
    }
}

/// Simple glob matcher reused from automerge (supports `*` and `**`).
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    glob_match_inner(&pattern, &text)
}

fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (None, _) => false,
        (Some('*'), _) => {
            if pattern.get(1) == Some(&'*') {
                let rest = &pattern[2..];
                let rest = if rest.first() == Some(&'/') { &rest[1..] } else { rest };
                for i in 0..=text.len() {
                    if glob_match_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                false
            } else {
                let rest = &pattern[1..];
                for i in 0..=text.len() {
                    if text[..i].iter().any(|&c| c == '/') {
                        break;
                    }
                    if glob_match_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                false
            }
        }
        (Some(&p), Some(&t)) if p == t => glob_match_inner(&pattern[1..], &text[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GroupBy, GroupingRule};
    use crate::manager::{Dependency, RegistrySource, UpdateContext};
    use crate::registry::VersionInfo;
    use semver::Version;

    fn make_candidate(
        name: &str,
        current: &str,
        new_ver: &str,
        file_path: &str,
        registry: RegistrySource,
    ) -> UpdateCandidate {
        let version = semver::Version::parse(new_ver.trim_start_matches('v'))
            .unwrap_or(semver::Version::new(0, 0, 0));
        UpdateCandidate {
            dependency: Dependency {
                name: name.to_string(),
                current_version: current.to_string(),
                registry,
                file_path: file_path.to_string(),
                update_context: UpdateContext::YamlKeyPath {
                    keys: vec!["tag".to_string()],
                },
            },
            new_version: VersionInfo {
                original_tag: new_ver.to_string(),
                version,
            },
            file_content: String::new(),
        }
    }

    fn docker_registry(image: &str) -> RegistrySource {
        RegistrySource::DockerRegistry {
            image: image.to_string(),
            registry: None,
        }
    }

    fn helm_registry(repo: &str, chart: &str) -> RegistrySource {
        RegistrySource::HelmRepository {
            repo_url: repo.to_string(),
            chart_name: chart.to_string(),
        }
    }

    #[test]
    fn per_dependency_default() {
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.26.0", "Dockerfile", docker_registry("nginx")),
            make_candidate("redis", "7.0.0", "7.2.0", "Dockerfile", docker_registry("redis")),
        ];
        let groups = group_candidates(candidates, &[], "per-dependency");
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|g| g.name.contains("nginx")));
        assert!(groups.iter().any(|g| g.name.contains("redis")));
        for g in &groups {
            assert_eq!(g.candidates.len(), 1);
        }
    }

    #[test]
    fn grouped_default_merges_unmatched() {
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.26.0", "Dockerfile", docker_registry("nginx")),
            make_candidate("redis", "7.0.0", "7.2.0", "Dockerfile", docker_registry("redis")),
        ];
        let groups = group_candidates(candidates, &[], "grouped");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "all");
        assert_eq!(groups[0].candidates.len(), 2);
    }

    #[test]
    fn rule_by_pattern_groups_all_matches() {
        let rule = GroupingRule {
            name: "infra".to_string(),
            match_patterns: vec!["nginx".to_string(), "redis".to_string()],
            group_by: GroupBy::Pattern,
            separate_major: false,
        };
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.26.0", "Dockerfile", docker_registry("nginx")),
            make_candidate("redis", "7.0.0", "7.2.0", "Dockerfile", docker_registry("redis")),
            make_candidate("postgres", "15.0.0", "16.0.0", "docker-compose.yaml", docker_registry("postgres")),
        ];
        let groups = group_candidates(candidates, &[rule], "per-dependency");
        // "infra" group + 1 per-dep group for postgres
        assert_eq!(groups.len(), 2);
        let infra = groups.iter().find(|g| g.name == "infra").unwrap();
        assert_eq!(infra.candidates.len(), 2);
    }

    #[test]
    fn rule_by_update_type() {
        let rule = GroupingRule {
            name: "deps".to_string(),
            match_patterns: vec![],
            group_by: GroupBy::UpdateType,
            separate_major: false,
        };
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.25.1", "Dockerfile", docker_registry("nginx")),
            make_candidate("redis", "7.0.0", "7.0.1", "Dockerfile", docker_registry("redis")),
            make_candidate("postgres", "15.0.0", "15.1.0", "Dockerfile", docker_registry("postgres")),
        ];
        let groups = group_candidates(candidates, &[rule], "per-dependency");
        // patch group + minor group
        assert_eq!(groups.len(), 2);
        let patch = groups.iter().find(|g| g.name == "deps-patch").unwrap();
        assert_eq!(patch.candidates.len(), 2);
        let minor = groups.iter().find(|g| g.name == "deps-minor").unwrap();
        assert_eq!(minor.candidates.len(), 1);
    }

    #[test]
    fn rule_by_manager() {
        let rule = GroupingRule {
            name: "all-deps".to_string(),
            match_patterns: vec![],
            group_by: GroupBy::Manager,
            separate_major: false,
        };
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.26.0", "Dockerfile", docker_registry("nginx")),
            make_candidate("my-chart", "0.1.0", "0.2.0", "charts/values.yaml", helm_registry("https://charts.example.com", "my-chart")),
        ];
        let groups = group_candidates(candidates, &[rule], "per-dependency");
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|g| g.name == "all-deps-docker"));
        assert!(groups.iter().any(|g| g.name == "all-deps-helm"));
    }

    #[test]
    fn separate_major_splits_into_own_group() {
        let rule = GroupingRule {
            name: "deps".to_string(),
            match_patterns: vec![],
            group_by: GroupBy::Pattern,
            separate_major: true,
        };
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.26.0", "Dockerfile", docker_registry("nginx")),
            make_candidate("redis", "7.0.0", "8.0.0", "Dockerfile", docker_registry("redis")),
        ];
        let groups = group_candidates(candidates, &[rule], "per-dependency");
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|g| g.name == "deps"));
        assert!(groups.iter().any(|g| g.name == "deps-major"));
        let major = groups.iter().find(|g| g.name == "deps-major").unwrap();
        assert_eq!(major.candidates[0].dependency.name, "redis");
    }

    #[test]
    fn rule_by_path() {
        let rule = GroupingRule {
            name: "by-path".to_string(),
            match_patterns: vec![],
            group_by: GroupBy::Path,
            separate_major: false,
        };
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.26.0", "services/web/Dockerfile", docker_registry("nginx")),
            make_candidate("redis", "7.0.0", "7.2.0", "services/cache/Dockerfile", docker_registry("redis")),
            make_candidate("postgres", "15.0.0", "16.0.0", "services/web/docker-compose.yaml", docker_registry("postgres")),
        ];
        let groups = group_candidates(candidates, &[rule], "per-dependency");
        assert_eq!(groups.len(), 2);
        let web = groups.iter().find(|g| g.name.contains("web")).unwrap();
        assert_eq!(web.candidates.len(), 2);
        let cache = groups.iter().find(|g| g.name.contains("cache")).unwrap();
        assert_eq!(cache.candidates.len(), 1);
    }

    #[test]
    fn first_matching_rule_wins() {
        let rule1 = GroupingRule {
            name: "nginx-group".to_string(),
            match_patterns: vec!["nginx".to_string()],
            group_by: GroupBy::Pattern,
            separate_major: false,
        };
        let rule2 = GroupingRule {
            name: "all".to_string(),
            match_patterns: vec![],
            group_by: GroupBy::Pattern,
            separate_major: false,
        };
        let candidates = vec![
            make_candidate("nginx", "1.25.0", "1.26.0", "Dockerfile", docker_registry("nginx")),
            make_candidate("redis", "7.0.0", "7.2.0", "Dockerfile", docker_registry("redis")),
        ];
        let groups = group_candidates(candidates, &[rule1, rule2], "per-dependency");
        assert_eq!(groups.len(), 2);
        let ng = groups.iter().find(|g| g.name == "nginx-group").unwrap();
        assert_eq!(ng.candidates.len(), 1);
        assert_eq!(ng.candidates[0].dependency.name, "nginx");
        let all = groups.iter().find(|g| g.name == "all").unwrap();
        assert_eq!(all.candidates.len(), 1);
        assert_eq!(all.candidates[0].dependency.name, "redis");
    }
}
