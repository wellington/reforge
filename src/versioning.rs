use semver::Version;

use crate::registry::VersionInfo;

#[derive(Debug, Clone)]
pub enum PinStrategy {
    SemverPatch,
    SemverMinor,
    SemverMajor,
}

impl PinStrategy {
    pub fn from_str(s: &str) -> Self {
        match s {
            "semver-patch" => Self::SemverPatch,
            "semver-major" => Self::SemverMajor,
            _ => Self::SemverMinor,
        }
    }
}

pub struct VersionPolicy {
    pub strategy: PinStrategy,
}

impl VersionPolicy {
    pub fn new(strategy: PinStrategy) -> Self {
        Self { strategy }
    }

    /// Given a current version and available versions, return the best update candidate.
    /// Returns None if already up to date.
    pub fn best_update(
        &self,
        current: &Version,
        available: &[VersionInfo],
    ) -> Option<VersionInfo> {
        let mut candidates: Vec<&VersionInfo> = available
            .iter()
            .filter(|v| {
                v.version > *current
                    && v.version.pre.is_empty()
                    && self.matches_strategy(current, &v.version)
            })
            .collect();

        candidates.sort_by(|a, b| b.version.cmp(&a.version));
        candidates.first().cloned().cloned()
    }

    fn matches_strategy(&self, current: &Version, candidate: &Version) -> bool {
        match self.strategy {
            PinStrategy::SemverPatch => {
                candidate.major == current.major && candidate.minor == current.minor
            }
            PinStrategy::SemverMinor => candidate.major == current.major,
            PinStrategy::SemverMajor => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::VersionInfo;

    fn vi(tag: &str) -> VersionInfo {
        VersionInfo {
            version: Version::parse(tag).unwrap(),
            original_tag: tag.to_string(),
        }
    }

    #[test]
    fn test_semver_minor_picks_latest_minor() {
        let policy = VersionPolicy::new(PinStrategy::SemverMinor);
        let current = Version::parse("1.25.0").unwrap();
        let available = vec![
            vi("1.25.1"),
            vi("1.26.0"),
            vi("1.24.0"),
            vi("2.0.0"),
        ];

        let best = policy.best_update(&current, &available).unwrap();
        assert_eq!(best.version, Version::parse("1.26.0").unwrap());
    }

    #[test]
    fn test_semver_patch_stays_in_minor() {
        let policy = VersionPolicy::new(PinStrategy::SemverPatch);
        let current = Version::parse("1.25.0").unwrap();
        let available = vec![vi("1.25.1"), vi("1.25.2"), vi("1.26.0")];

        let best = policy.best_update(&current, &available).unwrap();
        assert_eq!(best.version, Version::parse("1.25.2").unwrap());
    }

    #[test]
    fn test_semver_major_picks_highest() {
        let policy = VersionPolicy::new(PinStrategy::SemverMajor);
        let current = Version::parse("1.25.0").unwrap();
        let available = vec![vi("1.26.0"), vi("2.0.0"), vi("3.1.0")];

        let best = policy.best_update(&current, &available).unwrap();
        assert_eq!(best.version, Version::parse("3.1.0").unwrap());
    }

    #[test]
    fn test_already_up_to_date() {
        let policy = VersionPolicy::new(PinStrategy::SemverMinor);
        let current = Version::parse("1.26.0").unwrap();
        let available = vec![vi("1.25.0"), vi("1.25.1"), vi("1.26.0")];

        assert!(policy.best_update(&current, &available).is_none());
    }

    #[test]
    fn test_skips_prereleases() {
        let policy = VersionPolicy::new(PinStrategy::SemverMinor);
        let current = Version::parse("1.25.0").unwrap();
        let available = vec![
            vi("1.26.0-beta.1"),
            vi("1.25.1"),
        ];

        // 1.26.0-beta.1 won't parse cleanly with our vi() helper because
        // semver::Version::parse handles it, but it has a non-empty pre field.
        let available_with_pre = vec![
            VersionInfo {
                version: Version::parse("1.26.0-beta.1").unwrap(),
                original_tag: "1.26.0-beta.1".to_string(),
            },
            vi("1.25.1"),
        ];

        let best = policy.best_update(&current, &available_with_pre).unwrap();
        assert_eq!(best.version, Version::parse("1.25.1").unwrap());
    }
}
