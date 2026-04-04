use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, warn};

pub struct ChangelogFetcher {
    client: Client,
    github_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
}

impl ChangelogFetcher {
    pub fn new(github_token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            github_token,
        }
    }

    /// Fetch release notes from GitHub Releases API for versions between from_version and to_version.
    pub async fn fetch_github_release_notes(
        &self,
        owner: &str,
        repo: &str,
        from_version: &str,
        to_version: &str,
    ) -> Option<String> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases",
            owner, repo
        );

        let mut req = self
            .client
            .get(&url)
            .header("User-Agent", "reforge/0.1")
            .header("Accept", "application/vnd.github+json");

        if let Some(token) = &self.github_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                debug!("Failed to fetch GitHub releases for {}/{}: {}", owner, repo, e);
                return None;
            }
        };

        if !response.status().is_success() {
            debug!(
                "GitHub releases API returned {} for {}/{}",
                response.status(),
                owner,
                repo
            );
            return None;
        }

        let releases: Vec<GitHubRelease> = match response.json().await {
            Ok(r) => r,
            Err(e) => {
                debug!("Failed to parse GitHub releases for {}/{}: {}", owner, repo, e);
                return None;
            }
        };

        let from = strip_v_prefix(from_version);
        let to = strip_v_prefix(to_version);

        let relevant: Vec<String> = releases
            .into_iter()
            .filter(|r| {
                let tag = strip_v_prefix(&r.tag_name);
                is_version_in_range(tag, from, to)
            })
            .filter_map(|r| {
                r.body.filter(|b| !b.trim().is_empty()).map(|b| {
                    format!("### {}\n\n{}", r.tag_name, b.trim())
                })
            })
            .collect();

        if relevant.is_empty() {
            return None;
        }

        Some(relevant.join("\n\n---\n\n"))
    }

    /// Fetch CHANGELOG.md from the GitHub raw URL and extract the section between versions.
    pub async fn fetch_changelog_md(
        &self,
        repo_url: &str,
        from_version: &str,
        to_version: &str,
    ) -> Option<String> {
        let raw_url = github_raw_changelog_url(repo_url)?;

        let mut req = self
            .client
            .get(&raw_url)
            .header("User-Agent", "reforge/0.1");

        if let Some(token) = &self.github_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                debug!("Failed to fetch CHANGELOG.md from {}: {}", raw_url, e);
                return None;
            }
        };

        if !response.status().is_success() {
            debug!(
                "CHANGELOG.md fetch returned {} from {}",
                response.status(),
                raw_url
            );
            return None;
        }

        let text = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                debug!("Failed to read CHANGELOG.md body: {}", e);
                return None;
            }
        };

        extract_changelog_range(&text, from_version, to_version)
    }

    /// Coordinator: tries GitHub releases first, falls back to CHANGELOG.md.
    pub async fn fetch_release_notes(
        &self,
        dep_name: &str,
        registry_source: Option<&str>,
        from_version: &str,
        to_version: &str,
    ) -> Option<String> {
        // Try to extract owner/repo from the registry source or dep name.
        let (owner, repo) = match extract_github_owner_repo(dep_name, registry_source) {
            Some(pair) => pair,
            None => {
                debug!("Cannot determine GitHub owner/repo for {}", dep_name);
                return None;
            }
        };

        debug!(
            "Fetching release notes for {}/{} ({} -> {})",
            owner, repo, from_version, to_version
        );

        if let Some(notes) = self
            .fetch_github_release_notes(&owner, &repo, from_version, to_version)
            .await
        {
            return Some(notes);
        }

        warn!(
            "GitHub releases not found for {}/{}, trying CHANGELOG.md",
            owner, repo
        );

        let repo_url = format!("https://github.com/{}/{}", owner, repo);
        self.fetch_changelog_md(&repo_url, from_version, to_version)
            .await
    }
}

/// Truncate changelog text to max_length characters, appending a note if truncated.
pub fn truncate_changelog(text: &str, max_length: usize) -> String {
    if text.len() <= max_length {
        return text.to_string();
    }

    // Try to cut at a word boundary.
    let cut = &text[..max_length];
    let truncated = cut.rfind('\n').map_or(cut, |i| &cut[..i]);
    format!("{}\n\n... (truncated)", truncated)
}

/// Wrap changelog notes in a collapsible `<details>` block.
pub fn render_changelog_section(notes: &str) -> String {
    format!(
        "<details>\n<summary>Release Notes</summary>\n\n{}\n\n</details>",
        notes
    )
}

/// Extract the relevant section(s) of a CHANGELOG.md between from_version and to_version.
pub fn extract_changelog_range(
    changelog: &str,
    from_version: &str,
    to_version: &str,
) -> Option<String> {
    let from = strip_v_prefix(from_version);
    let to = strip_v_prefix(to_version);

    let lines: Vec<&str> = changelog.lines().collect();

    // Find heading lines that look like version headings (## or ###).
    // We collect sections whose version falls in the range (from, to].
    let mut sections: Vec<String> = Vec::new();
    let mut current_section_lines: Vec<&str> = Vec::new();
    let mut current_version: Option<String> = None;
    let mut capturing = false;

    let flush = |ver: &Option<String>,
                 section_lines: &[&str],
                 from: &str,
                 to: &str,
                 sections: &mut Vec<String>| {
        if let Some(v) = ver {
            let v_stripped = strip_v_prefix(v);
            if is_version_in_range(v_stripped, from, to) && !section_lines.is_empty() {
                sections.push(section_lines.join("\n"));
            }
        }
    };

    for line in &lines {
        if let Some(ver) = parse_version_heading(line) {
            flush(
                &current_version,
                &current_section_lines,
                from,
                to,
                &mut sections,
            );
            current_version = Some(ver.to_string());
            current_section_lines = vec![line];
            capturing = true;
        } else if capturing {
            current_section_lines.push(line);
        }
    }

    flush(
        &current_version,
        &current_section_lines,
        from,
        to,
        &mut sections,
    );

    if sections.is_empty() {
        return None;
    }

    Some(sections.join("\n\n---\n\n"))
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn strip_v_prefix(v: &str) -> &str {
    v.strip_prefix('v').unwrap_or(v)
}

/// Returns true if `version` is strictly greater than `from` and <= `to`
/// (simple lexicographic comparison — good enough for semver when zero-padded).
fn is_version_in_range(version: &str, from: &str, to: &str) -> bool {
    version_gt(version, from) && (version == to || version_gt(to, version))
}

fn version_gt(a: &str, b: &str) -> bool {
    version_parts(a) > version_parts(b)
}

fn version_parts(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Try to parse a version out of a Markdown heading line like `## 1.2.3` or `## [1.2.3]`.
fn parse_version_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches('#').trim();
    // Strip surrounding brackets if present: [1.2.3]
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.find(']').map(|i| &s[..i]))
        .unwrap_or(trimmed);
    // Must start with a digit or 'v' followed by a digit.
    let candidate = inner.split_whitespace().next()?;
    let stripped = strip_v_prefix(candidate);
    if stripped.chars().next()?.is_ascii_digit() {
        Some(candidate)
    } else {
        None
    }
}

/// Convert a GitHub HTML repo URL to a raw content URL for CHANGELOG.md.
fn github_raw_changelog_url(repo_url: &str) -> Option<String> {
    // Accept https://github.com/owner/repo (with optional trailing slash / .git)
    let base = repo_url
        .trim_end_matches('/')
        .trim_end_matches(".git");
    if !base.contains("github.com") {
        return None;
    }
    Some(format!(
        "{}/raw/HEAD/CHANGELOG.md",
        base
    ))
}

/// Attempt to derive (owner, repo) from a dependency name or registry source string.
fn extract_github_owner_repo<'a>(
    dep_name: &'a str,
    registry_source: Option<&'a str>,
) -> Option<(String, String)> {
    // Try the registry source first (may contain a full GitHub URL).
    if let Some(src) = registry_source {
        if let Some(pair) = parse_github_url(src) {
            return Some(pair);
        }
    }
    // Fall back to dep_name if it looks like "owner/repo".
    let parts: Vec<&str> = dep_name.splitn(2, '/').collect();
    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        return Some((parts[0].to_string(), parts[1].to_string()));
    }
    None
}

fn parse_github_url(s: &str) -> Option<(String, String)> {
    let s = s
        .trim_end_matches('/')
        .trim_end_matches(".git");
    // Expected shape: https://github.com/owner/repo[/...]
    let after_host = s.split("github.com/").nth(1)?;
    let mut parts = after_host.splitn(3, '/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_changelog_short() {
        let text = "short text";
        assert_eq!(truncate_changelog(text, 2000), text);
    }

    #[test]
    fn test_truncate_changelog_long() {
        let text = "a".repeat(3000);
        let result = truncate_changelog(&text, 2000);
        assert!(result.ends_with("... (truncated)"));
        assert!(result.len() < 3000);
    }

    #[test]
    fn test_truncate_changelog_prefers_newline_boundary() {
        let text = format!("{}\n{}", "a".repeat(1990), "b".repeat(100));
        let result = truncate_changelog(&text, 2000);
        assert!(result.ends_with("... (truncated)"));
        // Should have cut at the newline before the 'b's.
        assert!(!result.contains('b'));
    }

    #[test]
    fn test_render_changelog_section() {
        let notes = "### 1.2.0\n\nFixed a bug.";
        let rendered = render_changelog_section(notes);
        assert!(rendered.contains("<details>"));
        assert!(rendered.contains("</details>"));
        assert!(rendered.contains("Release Notes"));
        assert!(rendered.contains(notes));
    }

    #[test]
    fn test_extract_changelog_range_basic() {
        let changelog = "\
## 2.0.0
Breaking change.

## 1.2.0
New feature.

## 1.1.0
Bug fix.

## 1.0.0
Initial release.
";
        let result = extract_changelog_range(changelog, "1.1.0", "1.2.0");
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("1.2.0"));
        assert!(!text.contains("2.0.0"));
        assert!(!text.contains("1.1.0"));
    }

    #[test]
    fn test_extract_changelog_range_v_prefix() {
        let changelog = "\
## v1.3.0
Improvement.

## v1.2.0
Fix.

## v1.1.0
Old.
";
        let result = extract_changelog_range(changelog, "v1.1.0", "v1.3.0");
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("v1.3.0"));
        assert!(text.contains("v1.2.0"));
        assert!(!text.contains("v1.1.0"));
    }

    #[test]
    fn test_extract_changelog_range_no_match() {
        let changelog = "\
## 1.0.0
Initial release.
";
        let result = extract_changelog_range(changelog, "2.0.0", "3.0.0");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_github_url() {
        let pair = parse_github_url("https://github.com/helm/helm");
        assert_eq!(pair, Some(("helm".to_string(), "helm".to_string())));

        let pair2 = parse_github_url("https://github.com/org/repo.git");
        assert_eq!(pair2, Some(("org".to_string(), "repo".to_string())));

        let pair3 = parse_github_url("https://example.com/owner/repo");
        assert_eq!(pair3, None);
    }

    #[test]
    fn test_extract_github_owner_repo_from_dep_name() {
        let pair = extract_github_owner_repo("helm/helm", None);
        assert_eq!(pair, Some(("helm".to_string(), "helm".to_string())));
    }

    #[test]
    fn test_extract_github_owner_repo_from_registry_source() {
        let pair = extract_github_owner_repo(
            "helm",
            Some("https://github.com/helm/helm"),
        );
        assert_eq!(pair, Some(("helm".to_string(), "helm".to_string())));
    }

    #[test]
    fn test_is_version_in_range() {
        assert!(is_version_in_range("1.2.0", "1.1.0", "1.2.0"));
        assert!(!is_version_in_range("1.1.0", "1.1.0", "1.2.0"));
        assert!(!is_version_in_range("1.3.0", "1.1.0", "1.2.0"));
    }
}
