use serde::Deserialize;
use tracing::{info, warn};

use crate::error::Result;
use crate::platform::gitlab::{GitLabClient, MergeRequest};

/// How to handle MRs whose target branch has moved forward or that have conflicts.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StaleMrStrategy {
    /// Ask GitLab to rebase the MR branch onto the current target branch.
    #[default]
    Rebase,
    /// Delete and recreate the source branch from the current target branch,
    /// re-applying the dependency update commit.
    Recreate,
    /// Leave stale MRs untouched.
    Ignore,
}

/// An open MR that may be stale.
#[derive(Debug, Clone)]
pub struct StaleMr {
    pub mr: MergeRequest,
    pub has_conflicts: bool,
    pub is_behind: bool,
}

impl StaleMr {
    /// Returns true if any remediation is warranted.
    pub fn needs_action(&self) -> bool {
        self.has_conflicts || self.is_behind
    }
}

/// Checks open reforge MRs for staleness (conflicts or divergence).
pub struct StalenessChecker;

impl StalenessChecker {
    pub fn new() -> Self {
        Self
    }

    /// List open MRs with the given branch prefix and annotate each with
    /// staleness information by fetching the detailed MR view.
    pub async fn check_stale_mrs(
        &self,
        gitlab: &GitLabClient,
        project: &str,
        branch_prefix: &str,
    ) -> Vec<StaleMr> {
        let mrs = match gitlab
            .list_open_mrs(project, Some(branch_prefix))
            .await
        {
            Ok(mrs) => mrs,
            Err(e) => {
                warn!("Failed to list open MRs for staleness check: {}", e);
                return vec![];
            }
        };

        let mut stale = Vec::new();

        for mr in mrs {
            match gitlab.get_mr_detail(project, mr.iid).await {
                Ok(detail) => {
                    let has_conflicts = detail.has_conflicts;
                    let is_behind = detail.diverged_commits_count.unwrap_or(0) > 0;

                    if has_conflicts || is_behind {
                        info!(
                            "MR !{} '{}' is stale (conflicts={}, behind={})",
                            mr.iid, mr.title, has_conflicts, is_behind
                        );
                        stale.push(StaleMr {
                            mr,
                            has_conflicts,
                            is_behind,
                        });
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch detail for MR !{}: {}", mr.iid, e);
                }
            }
        }

        stale
    }
}

/// Request GitLab to rebase the MR's source branch onto its target branch.
pub async fn rebase_mr(gitlab: &GitLabClient, project: &str, mr_iid: u64) -> Result<()> {
    info!("Rebasing MR !{} via GitLab API", mr_iid);
    gitlab.rebase_mr(project, mr_iid).await
}

/// Recreate the source branch from `default_branch`, re-apply the update
/// commit, and preserve the existing MR (which follows the renamed branch
/// automatically because GitLab tracks source branch name, not SHA).
pub async fn recreate_mr_branch(
    gitlab: &GitLabClient,
    project: &str,
    mr: &MergeRequest,
    default_branch: &str,
    file_path: &str,
    updated_content: &str,
    commit_message: &str,
) -> Result<()> {
    info!(
        "Recreating branch '{}' for MR !{}",
        mr.source_branch, mr.iid
    );

    // Delete the old branch (ignore errors if it no longer exists).
    if let Err(e) = gitlab.delete_branch(project, &mr.source_branch).await {
        warn!(
            "Could not delete branch '{}': {} — continuing",
            mr.source_branch, e
        );
    }

    // Re-create the branch from the current tip of the default branch.
    gitlab
        .create_branch(project, &mr.source_branch, default_branch)
        .await?;

    // Re-apply the update.
    use crate::platform::gitlab::CommitAction;
    gitlab
        .commit_files(
            project,
            &mr.source_branch,
            commit_message,
            vec![CommitAction {
                action: "update".to_string(),
                file_path: file_path.to_string(),
                content: updated_content.to_string(),
            }],
        )
        .await?;

    info!(
        "Branch '{}' recreated for MR !{}",
        mr.source_branch, mr.iid
    );
    Ok(())
}

/// Apply the configured strategy to a collection of stale MRs (GitLab mode).
pub async fn handle_stale_mrs(
    gitlab: &GitLabClient,
    project: &str,
    stale_mrs: &[StaleMr],
    strategy: &StaleMrStrategy,
) {
    if stale_mrs.is_empty() {
        return;
    }

    match strategy {
        StaleMrStrategy::Ignore => {
            info!(
                "Staleness strategy is 'ignore' — leaving {} stale MR(s) untouched",
                stale_mrs.len()
            );
        }
        StaleMrStrategy::Rebase => {
            for stale in stale_mrs {
                if let Err(e) = rebase_mr(gitlab, project, stale.mr.iid).await {
                    warn!("Failed to rebase MR !{}: {}", stale.mr.iid, e);
                }
            }
        }
        StaleMrStrategy::Recreate => {
            // Recreate requires the caller to supply file content; log a warning
            // because that information is not available at this call site without
            // re-scanning. Users should prefer Rebase for GitLab mode.
            warn!(
                "Recreate strategy in GitLab mode requires re-scan context; \
                 falling back to rebase for {} MR(s)",
                stale_mrs.len()
            );
            for stale in stale_mrs {
                if let Err(e) = rebase_mr(gitlab, project, stale.mr.iid).await {
                    warn!("Failed to rebase MR !{}: {}", stale.mr.iid, e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stale_mr_strategy_default_is_rebase() {
        let strategy = StaleMrStrategy::default();
        assert_eq!(strategy, StaleMrStrategy::Rebase);
    }

    #[test]
    fn test_stale_mr_strategy_deserialize() {
        #[derive(Deserialize)]
        struct Wrapper {
            strategy: StaleMrStrategy,
        }

        let w: Wrapper = toml::from_str("strategy = \"rebase\"").unwrap();
        assert_eq!(w.strategy, StaleMrStrategy::Rebase);

        let w: Wrapper = toml::from_str("strategy = \"recreate\"").unwrap();
        assert_eq!(w.strategy, StaleMrStrategy::Recreate);

        let w: Wrapper = toml::from_str("strategy = \"ignore\"").unwrap();
        assert_eq!(w.strategy, StaleMrStrategy::Ignore);
    }

    #[test]
    fn test_stale_mr_needs_action_conflicts() {
        let mr = make_mr();
        let stale = StaleMr {
            mr,
            has_conflicts: true,
            is_behind: false,
        };
        assert!(stale.needs_action());
    }

    #[test]
    fn test_stale_mr_needs_action_behind() {
        let mr = make_mr();
        let stale = StaleMr {
            mr,
            has_conflicts: false,
            is_behind: true,
        };
        assert!(stale.needs_action());
    }

    #[test]
    fn test_stale_mr_no_action_needed() {
        let mr = make_mr();
        let stale = StaleMr {
            mr,
            has_conflicts: false,
            is_behind: false,
        };
        assert!(!stale.needs_action());
    }

    fn make_mr() -> MergeRequest {
        MergeRequest {
            iid: 1,
            title: "test".to_string(),
            source_branch: "reforge/test".to_string(),
            target_branch: "main".to_string(),
            state: "opened".to_string(),
            web_url: "https://gitlab.example.com/mr/1".to_string(),
        }
    }
}
