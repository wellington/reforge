use chrono::{DateTime, Utc};
use semver::Version;

use crate::config::{AutomergePolicy, UpdateTypeFilter};

/// Semver bump classification for a dependency update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateType {
    Patch,
    Minor,
    Major,
}

impl UpdateType {
    /// Classify the bump between two semver strings. Returns `None` if either
    /// string cannot be parsed as a valid semver version.
    pub fn classify(current: &str, new: &str) -> Option<Self> {
        let current = parse_version(current)?;
        let new = parse_version(new)?;

        if new.major != current.major {
            Some(UpdateType::Major)
        } else if new.minor != current.minor {
            Some(UpdateType::Minor)
        } else {
            Some(UpdateType::Patch)
        }
    }
}

fn parse_version(s: &str) -> Option<Version> {
    // Strip a leading 'v' that is common in Docker/Helm tags.
    let s = s.trim_start_matches('v');
    Version::parse(s).ok()
}

/// Evaluates automerge policies for a given dependency update.
pub struct AutomergeEvaluator<'a> {
    policies: &'a [AutomergePolicy],
}

impl<'a> AutomergeEvaluator<'a> {
    pub fn new(policies: &'a [AutomergePolicy]) -> Self {
        Self { policies }
    }

    /// Returns `true` when a matching policy enables automerge for this update.
    ///
    /// `mr_created_at` is used to enforce `minimum_age_days`; pass `None` to
    /// skip age checking (e.g. in local mode where there is no real MR yet).
    pub fn should_automerge(
        &self,
        dep_name: &str,
        update_type: &UpdateType,
        mr_created_at: Option<DateTime<Utc>>,
    ) -> bool {
        for policy in self.policies {
            if !pattern_matches(&policy.match_pattern, dep_name) {
                continue;
            }

            if !policy.update_types.is_empty()
                && !policy.update_types.contains(&update_type_to_filter(update_type))
            {
                continue;
            }

            if !policy.enabled {
                return false;
            }

            if let Some(min_age) = policy.minimum_age_days {
                if let Some(created_at) = mr_created_at {
                    let age_days = (Utc::now() - created_at).num_days();
                    if age_days < min_age as i64 {
                        return false;
                    }
                }
            }

            return true;
        }

        false
    }
}

fn update_type_to_filter(ut: &UpdateType) -> UpdateTypeFilter {
    match ut {
        UpdateType::Patch => UpdateTypeFilter::Patch,
        UpdateType::Minor => UpdateTypeFilter::Minor,
        UpdateType::Major => UpdateTypeFilter::Major,
    }
}

/// Glob matcher supporting `*` (any substring, not crossing `/`) and `**` (any substring).
fn pattern_matches(pattern: &str, name: &str) -> bool {
    crate::util::glob_match(pattern, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AutomergePolicy, UpdateTypeFilter};

    // ── UpdateType::classify ────────────────────────────────────────────────

    #[test]
    fn classify_patch() {
        assert_eq!(
            UpdateType::classify("1.2.3", "1.2.4"),
            Some(UpdateType::Patch)
        );
    }

    #[test]
    fn classify_minor() {
        assert_eq!(
            UpdateType::classify("1.2.3", "1.3.0"),
            Some(UpdateType::Minor)
        );
    }

    #[test]
    fn classify_major() {
        assert_eq!(
            UpdateType::classify("1.2.3", "2.0.0"),
            Some(UpdateType::Major)
        );
    }

    #[test]
    fn classify_with_v_prefix() {
        assert_eq!(
            UpdateType::classify("v1.2.3", "v1.2.9"),
            Some(UpdateType::Patch)
        );
        assert_eq!(
            UpdateType::classify("v1.2.3", "v2.0.0"),
            Some(UpdateType::Major)
        );
    }

    #[test]
    fn classify_unparseable_returns_none() {
        assert_eq!(UpdateType::classify("latest", "1.2.3"), None);
        assert_eq!(UpdateType::classify("1.2.3", "nightly"), None);
    }

    // ── pattern_matches ─────────────────────────────────────────────────────

    #[test]
    fn pattern_exact_match() {
        assert!(pattern_matches("nginx", "nginx"));
        assert!(!pattern_matches("nginx", "redis"));
    }

    #[test]
    fn pattern_wildcard_all() {
        assert!(pattern_matches("*", "anything"));
        assert!(pattern_matches("**", "anything/nested"));
    }

    #[test]
    fn pattern_prefix_glob() {
        assert!(pattern_matches("nginx*", "nginx"));
        assert!(pattern_matches("nginx*", "nginx-proxy"));
        assert!(!pattern_matches("nginx*", "redis"));
    }

    #[test]
    fn pattern_glob_with_slash() {
        assert!(pattern_matches("myorg/**", "myorg/nginx"));
        assert!(pattern_matches("myorg/**", "myorg/team/nginx"));
        assert!(!pattern_matches("myorg/**", "other/nginx"));
    }

    // ── AutomergeEvaluator ──────────────────────────────────────────────────

    fn make_policy(
        pattern: &str,
        update_types: Vec<UpdateTypeFilter>,
        enabled: bool,
        minimum_age_days: Option<u32>,
    ) -> AutomergePolicy {
        AutomergePolicy {
            match_pattern: pattern.to_string(),
            update_types,
            enabled,
            minimum_age_days,
        }
    }

    #[test]
    fn automerge_patch_only_policy() {
        let policies = vec![make_policy(
            "nginx",
            vec![UpdateTypeFilter::Patch],
            true,
            None,
        )];
        let eval = AutomergeEvaluator::new(&policies);

        assert!(eval.should_automerge("nginx", &UpdateType::Patch, None));
        assert!(!eval.should_automerge("nginx", &UpdateType::Minor, None));
        assert!(!eval.should_automerge("nginx", &UpdateType::Major, None));
    }

    #[test]
    fn automerge_any_update_type_when_list_empty() {
        let policies = vec![make_policy("nginx", vec![], true, None)];
        let eval = AutomergeEvaluator::new(&policies);

        assert!(eval.should_automerge("nginx", &UpdateType::Patch, None));
        assert!(eval.should_automerge("nginx", &UpdateType::Minor, None));
        assert!(eval.should_automerge("nginx", &UpdateType::Major, None));
    }

    #[test]
    fn automerge_disabled_policy() {
        let policies = vec![make_policy("nginx", vec![], false, None)];
        let eval = AutomergeEvaluator::new(&policies);
        assert!(!eval.should_automerge("nginx", &UpdateType::Patch, None));
    }

    #[test]
    fn automerge_no_matching_policy() {
        let policies = vec![make_policy("redis", vec![], true, None)];
        let eval = AutomergeEvaluator::new(&policies);
        assert!(!eval.should_automerge("nginx", &UpdateType::Patch, None));
    }

    #[test]
    fn automerge_minimum_age_not_met() {
        use chrono::Duration;
        let policies = vec![make_policy("nginx", vec![], true, Some(3))];
        let eval = AutomergeEvaluator::new(&policies);

        // MR created 1 day ago — age not met
        let created_at = Utc::now() - Duration::days(1);
        assert!(!eval.should_automerge("nginx", &UpdateType::Patch, Some(created_at)));
    }

    #[test]
    fn automerge_minimum_age_met() {
        use chrono::Duration;
        let policies = vec![make_policy("nginx", vec![], true, Some(3))];
        let eval = AutomergeEvaluator::new(&policies);

        // MR created 5 days ago — age met
        let created_at = Utc::now() - Duration::days(5);
        assert!(eval.should_automerge("nginx", &UpdateType::Patch, Some(created_at)));
    }

    #[test]
    fn automerge_first_matching_policy_wins() {
        let policies = vec![
            make_policy("nginx", vec![UpdateTypeFilter::Patch], true, None),
            make_policy("nginx", vec![], false, None), // would disable all if first didn't match
        ];
        let eval = AutomergeEvaluator::new(&policies);

        assert!(eval.should_automerge("nginx", &UpdateType::Patch, None));
        // Minor doesn't match first policy, falls through to second (disabled)
        assert!(!eval.should_automerge("nginx", &UpdateType::Minor, None));
    }

    #[test]
    fn automerge_wildcard_policy() {
        let policies = vec![make_policy(
            "*",
            vec![UpdateTypeFilter::Patch],
            true,
            None,
        )];
        let eval = AutomergeEvaluator::new(&policies);

        assert!(eval.should_automerge("nginx", &UpdateType::Patch, None));
        assert!(eval.should_automerge("redis", &UpdateType::Patch, None));
        assert!(!eval.should_automerge("nginx", &UpdateType::Major, None));
    }
}
