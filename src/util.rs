//! Shared utility functions.
//!
//! Contains glob pattern matching and image reference parsing used
//! across multiple modules.

use crate::manager::RegistrySource;

/// Matches a glob pattern against text.
///
/// Supports:
/// - `*` — matches any sequence of characters **not** including `/`
/// - `**` — matches any sequence of characters **including** `/`
/// - Any other character — matches exactly
///
/// # Examples
/// ```ignore
/// assert!(glob_match("*.yaml", "values.yaml"));
/// assert!(glob_match("apps/**/*.yaml", "apps/web/config.yaml"));
/// assert!(!glob_match("*.yaml", "subdir/values.yaml"));
/// ```
pub fn glob_match(pattern: &str, text: &str) -> bool {
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
                // `**` — consume any number of characters including '/'
                let rest = &pattern[2..];
                let rest = if rest.first() == Some(&'/') { &rest[1..] } else { rest };
                for i in 0..=text.len() {
                    if glob_match_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                false
            } else {
                // `*` — consume any number of characters except '/'
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

/// Returns the manager name for a registry source.
///
/// Used for branch naming and MR grouping.
pub fn manager_name(registry: &RegistrySource) -> &'static str {
    match registry {
        RegistrySource::DockerRegistry { .. } => "docker",
        RegistrySource::HelmRepository { .. } | RegistrySource::OciHelmRegistry { .. } => "helm",
    }
}

/// Parses a Docker image reference into `(registry, name)` components.
///
/// Returns `Some(registry)` only when the prefix looks like a registry host
/// (contains `.` or `:`). Plain Docker Hub references like `library/nginx`
/// return `(None, "library/nginx")`.
///
/// # Examples
/// ```ignore
/// assert_eq!(parse_image_reference("nginx"), (None, "nginx".into()));
/// assert_eq!(parse_image_reference("ghcr.io/owner/app"), (Some("ghcr.io/owner".into()), "app".into()));
/// ```
pub fn parse_image_reference(image: &str) -> (Option<String>, String) {
    if let Some(idx) = image.rfind('/') {
        let prefix = &image[..idx];
        let name = &image[idx + 1..];
        if prefix.contains('.') || prefix.contains(':') {
            return (Some(prefix.to_string()), name.to_string());
        }
        // Looks like a Docker Hub org/image — no distinct registry prefix
        (None, image.to_string())
    } else {
        (None, image.to_string())
    }
}
