use std::collections::HashMap;
use tracing::{debug, info};

use crate::error::Result;
use crate::manager::{Dependency, RegistrySource};
use crate::platform::gitlab::{GitLabClient, Issue, MergeRequest};
use crate::orchestrator::UpdateCandidate;

pub const DASHBOARD_TITLE: &str = "Dependency Dashboard";
const DASHBOARD_MARKER: &str = "<!-- reforge-dashboard -->";

/// Summary of a single dependency for dashboard display.
#[derive(Debug, Clone)]
pub struct DependencyStatus {
    pub name: String,
    pub manager: String,
    pub current_version: String,
    pub new_version: Option<String>,
    pub file_path: String,
    pub mr_url: Option<String>,
    pub mr_iid: Option<u64>,
    pub state: DependencyState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DependencyState {
    UpToDate,
    PendingUpdate,
    OpenMr,
}

/// Build dependency statuses from extracted deps and candidates.
pub fn build_statuses(
    all_deps: &[(Dependency, String)],
    candidates: &[UpdateCandidate],
    open_mrs: &[MergeRequest],
    branch_prefix: &str,
) -> Vec<DependencyStatus> {
    let candidate_map: HashMap<&str, &UpdateCandidate> = candidates
        .iter()
        .map(|c| (c.dependency.name.as_str(), c))
        .collect();

    let mr_by_branch: HashMap<&str, &MergeRequest> = open_mrs
        .iter()
        .map(|mr| (mr.source_branch.as_str(), mr))
        .collect();

    let mut statuses = Vec::new();

    for (dep, _content) in all_deps {
        let manager = manager_name(&dep.registry);

        let branch_name = {
            let sanitized = dep.name.replace('/', "-");
            if let Some(candidate) = candidate_map.get(dep.name.as_str()) {
                format!(
                    "{}{}-{}-{}",
                    branch_prefix,
                    manager,
                    sanitized,
                    candidate.new_version.original_tag
                )
            } else {
                String::new()
            }
        };

        let open_mr = if !branch_name.is_empty() {
            mr_by_branch.get(branch_name.as_str()).copied()
        } else {
            None
        };

        let status = if let Some(mr) = open_mr {
            DependencyStatus {
                name: dep.name.clone(),
                manager: manager.to_string(),
                current_version: dep.current_version.clone(),
                new_version: candidate_map
                    .get(dep.name.as_str())
                    .map(|c| c.new_version.original_tag.clone()),
                file_path: dep.file_path.clone(),
                mr_url: Some(mr.web_url.clone()),
                mr_iid: Some(mr.iid),
                state: DependencyState::OpenMr,
            }
        } else if let Some(candidate) = candidate_map.get(dep.name.as_str()) {
            DependencyStatus {
                name: dep.name.clone(),
                manager: manager.to_string(),
                current_version: dep.current_version.clone(),
                new_version: Some(candidate.new_version.original_tag.clone()),
                file_path: dep.file_path.clone(),
                mr_url: None,
                mr_iid: None,
                state: DependencyState::PendingUpdate,
            }
        } else {
            DependencyStatus {
                name: dep.name.clone(),
                manager: manager.to_string(),
                current_version: dep.current_version.clone(),
                new_version: None,
                file_path: dep.file_path.clone(),
                mr_url: None,
                mr_iid: None,
                state: DependencyState::UpToDate,
            }
        };

        statuses.push(status);
    }

    statuses.sort_by(|a, b| {
        let order = |s: &DependencyState| match s {
            DependencyState::PendingUpdate => 0,
            DependencyState::OpenMr => 1,
            DependencyState::UpToDate => 2,
        };
        order(&a.state)
            .cmp(&order(&b.state))
            .then(a.name.cmp(&b.name))
    });

    statuses
}

/// Render the dashboard issue body as markdown.
pub fn render_dashboard(statuses: &[DependencyStatus], project_label: &str) -> String {
    let mut body = String::new();

    body.push_str(DASHBOARD_MARKER);
    body.push('\n');
    body.push_str(&format!(
        "## Dependency Dashboard — {}\n\n",
        project_label
    ));
    body.push_str(
        "This issue lists all dependencies tracked by **reforge**. \
        Check a box next to a pending update to trigger its MR immediately.\n\n",
    );

    let pending: Vec<_> = statuses
        .iter()
        .filter(|s| s.state == DependencyState::PendingUpdate)
        .collect();
    let open_mr: Vec<_> = statuses
        .iter()
        .filter(|s| s.state == DependencyState::OpenMr)
        .collect();
    let up_to_date: Vec<_> = statuses
        .iter()
        .filter(|s| s.state == DependencyState::UpToDate)
        .collect();

    if !pending.is_empty() {
        body.push_str("### Pending Updates\n\n");
        body.push_str("Check a box to create an MR for that dependency on the next run.\n\n");
        body.push_str("| | Package | Manager | File | Current | Available |\n");
        body.push_str("|---|---------|---------|------|---------|----------|\n");
        for s in &pending {
            let new_ver = s.new_version.as_deref().unwrap_or("?");
            body.push_str(&format!(
                "| - [ ] | `{}` | {} | `{}` | {} | **{}** |\n",
                s.name, s.manager, s.file_path, s.current_version, new_ver,
            ));
        }
        body.push('\n');
    }

    if !open_mr.is_empty() {
        body.push_str("### Open MRs\n\n");
        body.push_str("| Package | Manager | File | Current | New | MR |\n");
        body.push_str("|---------|---------|------|---------|-----|----|\n");
        for s in &open_mr {
            let new_ver = s.new_version.as_deref().unwrap_or("?");
            let mr_link = match (&s.mr_url, &s.mr_iid) {
                (Some(url), Some(iid)) => format!("[!{}]({})", iid, url),
                _ => "-".to_string(),
            };
            body.push_str(&format!(
                "| `{}` | {} | `{}` | {} | **{}** | {} |\n",
                s.name, s.manager, s.file_path, s.current_version, new_ver, mr_link,
            ));
        }
        body.push('\n');
    }

    if !up_to_date.is_empty() {
        body.push_str("<details>\n<summary>Up to date</summary>\n\n");
        body.push_str("| Package | Manager | File | Version |\n");
        body.push_str("|---------|---------|------|--------|\n");
        for s in &up_to_date {
            body.push_str(&format!(
                "| `{}` | {} | `{}` | {} |\n",
                s.name, s.manager, s.file_path, s.current_version,
            ));
        }
        body.push_str("\n</details>\n\n");
    }

    body.push_str("---\n\n*Managed by [reforge](https://github.com/example/reforge). Do not edit the sections above manually.*\n");

    body
}

/// Upsert the dashboard issue on GitLab (create if absent, update if present).
pub async fn upsert_gitlab_dashboard(
    gitlab: &GitLabClient,
    project: &str,
    body: &str,
    labels: &[String],
) -> Result<Issue> {
    let existing = gitlab
        .list_issues(project, Some(DASHBOARD_TITLE), Some("opened"))
        .await?;

    let dashboard_issue = existing
        .into_iter()
        .find(|i| i.title == DASHBOARD_TITLE);

    if let Some(issue) = dashboard_issue {
        info!("Updating existing dashboard issue #{}", issue.iid);
        gitlab
            .update_issue(project, issue.iid, body)
            .await?;
        // Return a refreshed copy with the new description
        let mut updated = issue;
        updated.description = Some(body.to_string());
        Ok(updated)
    } else {
        info!("Creating new dashboard issue");
        gitlab
            .create_issue(project, DASHBOARD_TITLE, body, labels)
            .await
    }
}

/// Write the dashboard to a local markdown file.
pub fn write_local_dashboard(body: &str, path: &str) -> Result<()> {
    std::fs::write(path, body).map_err(|e| {
        crate::error::ReforgeError::Config(format!("Failed to write dashboard to {}: {}", path, e))
    })?;
    debug!("Wrote dashboard to {}", path);
    Ok(())
}

fn manager_name(registry: &RegistrySource) -> &'static str {
    crate::util::manager_name(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_status(
        name: &str,
        manager: &str,
        current: &str,
        new_ver: Option<&str>,
        state: DependencyState,
        mr_url: Option<&str>,
        mr_iid: Option<u64>,
    ) -> DependencyStatus {
        DependencyStatus {
            name: name.to_string(),
            manager: manager.to_string(),
            current_version: current.to_string(),
            new_version: new_ver.map(|s| s.to_string()),
            file_path: "chart/values.yaml".to_string(),
            mr_url: mr_url.map(|s| s.to_string()),
            mr_iid,
            state,
        }
    }

    #[test]
    fn render_empty_dashboard() {
        let body = render_dashboard(&[], "my-group/my-project");
        assert!(body.contains(DASHBOARD_MARKER));
        assert!(body.contains("Dependency Dashboard"));
        assert!(!body.contains("Pending Updates"));
        assert!(!body.contains("Open MRs"));
    }

    #[test]
    fn render_pending_updates_table() {
        let statuses = vec![make_status(
            "nginx",
            "docker",
            "1.24.0",
            Some("1.25.0"),
            DependencyState::PendingUpdate,
            None,
            None,
        )];
        let body = render_dashboard(&statuses, "my-project");
        assert!(body.contains("### Pending Updates"));
        assert!(body.contains("`nginx`"));
        assert!(body.contains("1.24.0"));
        assert!(body.contains("**1.25.0**"));
        assert!(body.contains("- [ ]"));
    }

    #[test]
    fn render_open_mr_with_link() {
        let statuses = vec![make_status(
            "alpine",
            "docker",
            "3.17.0",
            Some("3.18.0"),
            DependencyState::OpenMr,
            Some("https://gitlab.com/group/proj/-/merge_requests/42"),
            Some(42),
        )];
        let body = render_dashboard(&statuses, "my-project");
        assert!(body.contains("### Open MRs"));
        assert!(body.contains("[!42]"));
        assert!(body.contains("https://gitlab.com/group/proj/-/merge_requests/42"));
    }

    #[test]
    fn render_up_to_date_in_details() {
        let statuses = vec![make_status(
            "redis",
            "docker",
            "7.0.0",
            None,
            DependencyState::UpToDate,
            None,
            None,
        )];
        let body = render_dashboard(&statuses, "my-project");
        assert!(body.contains("<details>"));
        assert!(body.contains("`redis`"));
        assert!(!body.contains("### Pending Updates"));
    }

    #[test]
    fn render_mixed_statuses_ordering() {
        let statuses = vec![
            make_status(
                "zz-last",
                "helm",
                "1.0.0",
                None,
                DependencyState::UpToDate,
                None,
                None,
            ),
            make_status(
                "aa-first",
                "docker",
                "2.0.0",
                Some("3.0.0"),
                DependencyState::PendingUpdate,
                None,
                None,
            ),
            make_status(
                "bb-mr",
                "docker",
                "1.0.0",
                Some("1.1.0"),
                DependencyState::OpenMr,
                Some("https://gitlab.com/-/mr/1"),
                Some(1),
            ),
        ];
        let body = render_dashboard(&statuses, "project");
        let pending_pos = body.find("### Pending Updates").unwrap();
        let mr_pos = body.find("### Open MRs").unwrap();
        let details_pos = body.find("<details>").unwrap();
        assert!(pending_pos < mr_pos);
        assert!(mr_pos < details_pos);
    }

    #[test]
    fn dashboard_contains_reforge_marker() {
        let body = render_dashboard(&[], "project");
        assert!(body.starts_with(DASHBOARD_MARKER));
    }
}
