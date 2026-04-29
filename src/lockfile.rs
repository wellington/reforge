//! Helm Chart.lock file parsing and generation.
//!
//! When updating Helm chart dependencies in Chart.yaml, the companion
//! Chart.lock file must also be updated with the new version and digest.
//! This module handles:
//! - Parsing existing Chart.lock files
//! - Updating individual dependency versions and digests
//! - Fetching chart digests from HTTP or OCI registries

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::error::{ReforgeError, Result};
use crate::manager::RegistrySource;

/// Represents a Helm Chart.lock file.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct ChartLock {
    pub dependencies: Vec<ChartLockDependency>,
    pub digest: String,
    pub generated: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct ChartLockDependency {
    pub name: String,
    pub repository: String,
    pub version: String,
}

#[allow(dead_code)]
pub fn parse_chart_lock(contents: &str) -> Result<ChartLock> {
    serde_yaml::from_str(contents).map_err(|e| ReforgeError::Parse {
        file: "Chart.lock".to_string(),
        reason: format!("Failed to parse Chart.lock: {}", e),
    })
}

/// Generate a Chart.lock YAML string from a list of chart dependencies and their digests.
///
/// The `digests` map is keyed by dependency name. The overall lock file digest
/// is computed from the sorted dependency digests to be deterministic.
#[allow(dead_code)]
pub fn generate_chart_lock(
    deps: &[ChartLockDependency],
    digests: &HashMap<String, String>,
) -> String {
    let generated = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

    // Build a reproducible digest from all per-chart digests sorted by name.
    let mut sorted_names: Vec<&String> = digests.keys().collect();
    sorted_names.sort();
    let combined: String = sorted_names
        .iter()
        .map(|n| format!("{}:{}", n, digests[*n]))
        .collect::<Vec<_>>()
        .join(",");
    let overall_digest = sha256_hex(combined.as_bytes());

    let mut lines = Vec::new();
    lines.push("dependencies:".to_string());
    for dep in deps {
        lines.push(format!("- digest: {}", digests.get(&dep.name).map(String::as_str).unwrap_or("")));
        lines.push(format!("  name: {}", dep.name));
        lines.push(format!("  repository: {}", dep.repository));
        lines.push(format!("  version: {}", dep.version));
    }
    lines.push(format!("digest: sha256:{}", overall_digest));
    lines.push(format!("generated: \"{}\"", generated));
    lines.push(String::new());

    lines.join("\n")
}

/// Update a single dependency's version and digest in an existing Chart.lock string.
///
/// Uses string-based line scanning to preserve formatting. The function finds
/// the dependency block by matching the `name:` field and replaces the `version:`
/// and `digest:` values within that block.
pub fn update_chart_lock(
    existing_lock: &str,
    updated_dep_name: &str,
    new_version: &str,
    new_digest: &str,
) -> String {
    let lines: Vec<&str> = existing_lock.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());

    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Detect start of a dependency block.
        if trimmed == "dependencies:" {
            result.push(line.to_string());
            i += 1;
            continue;
        }

        // Detect a dependency list item that starts with `- `.
        if trimmed.starts_with("- ") {
            // Collect the full block (lines belonging to this list item).
            let mut block_lines: Vec<&str> = vec![line];
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                let next_trim = next.trim();
                // A new list item or a top-level key ends the block.
                if next_trim.starts_with("- ") || (!next_trim.is_empty() && !next_trim.starts_with('#') && !next.starts_with(' ') && !next.starts_with('\t')) {
                    break;
                }
                block_lines.push(next);
                i += 1;
            }

            // Check if this block belongs to the target dependency.
            let is_target = block_lines.iter().any(|bl| {
                let t = bl.trim();
                t == format!("name: {}", updated_dep_name)
                    || t.starts_with(&format!("name: {}", updated_dep_name))
            });

            if is_target {
                for bl in &block_lines {
                    let trimmed_start = bl.trim_start();
                    // Strip leading "- " for list items to get the bare key.
                    let bare = trimmed_start.trim_start_matches("- ").trim_start();
                    if bare.starts_with("version:") {
                        // Preserve everything before the `version` key.
                        let key_pos = bl.find("version:").unwrap();
                        result.push(format!("{}version: {}", &bl[..key_pos], new_version));
                    } else if bare.starts_with("digest:") {
                        let key_pos = bl.find("digest:").unwrap();
                        result.push(format!("{}digest: {}", &bl[..key_pos], new_digest));
                    } else {
                        result.push(bl.to_string());
                    }
                }
            } else {
                for bl in &block_lines {
                    result.push(bl.to_string());
                }
            }

            continue;
        }

        result.push(line.to_string());
        i += 1;
    }

    let mut out = result.join("\n");
    // Preserve trailing newline if the original had one.
    if existing_lock.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Fetch the digest for a chart from a Helm HTTP repo or OCI registry.
///
/// For HTTP repos: downloads the chart `.tgz` and computes its SHA256.
/// For OCI repos: queries the manifest `Content-Digest` header.
pub async fn fetch_chart_digest(
    client: &reqwest::Client,
    registry_source: &RegistrySource,
    version: &str,
) -> Result<String> {
    match registry_source {
        RegistrySource::HelmRepository {
            repo_url,
            chart_name,
        } => fetch_helm_http_digest(client, repo_url, chart_name, version).await,
        RegistrySource::OciHelmRegistry { image, registry } => {
            fetch_oci_digest(client, image, registry.as_deref(), version).await
        }
        RegistrySource::DockerRegistry { .. } => Err(ReforgeError::Registry {
            registry: "unknown".to_string(),
            message: "fetch_chart_digest called with a DockerRegistry source".to_string(),
        }),
    }
}

async fn fetch_helm_http_digest(
    client: &reqwest::Client,
    repo_url: &str,
    chart_name: &str,
    version: &str,
) -> Result<String> {
    // First, check the Helm index to get the chart URL.
    let index_url = format!("{}/index.yaml", repo_url.trim_end_matches('/'));
    debug!("Fetching Helm index for digest from {}", index_url);

    let index_resp = client
        .get(&index_url)
        .send()
        .await
        .map_err(|e| ReforgeError::Registry {
            registry: repo_url.to_string(),
            message: format!("Failed to fetch index.yaml: {}", e),
        })?;

    if !index_resp.status().is_success() {
        return Err(ReforgeError::Registry {
            registry: repo_url.to_string(),
            message: format!("index.yaml returned {}", index_resp.status()),
        });
    }

    let index_body = index_resp.text().await?;

    // Parse the index to find the chart URL for the requested version.
    let chart_url = extract_chart_url_from_index(&index_body, chart_name, version, repo_url)?;

    debug!("Downloading chart from {} to compute digest", chart_url);

    let chart_resp = client
        .get(&chart_url)
        .send()
        .await
        .map_err(|e| ReforgeError::Registry {
            registry: repo_url.to_string(),
            message: format!("Failed to download chart: {}", e),
        })?;

    if !chart_resp.status().is_success() {
        return Err(ReforgeError::Registry {
            registry: repo_url.to_string(),
            message: format!("Chart download returned {}", chart_resp.status()),
        });
    }

    let bytes = chart_resp.bytes().await?;
    let digest = sha256_hex(&bytes);
    Ok(format!("sha256:{}", digest))
}

fn extract_chart_url_from_index(
    index_yaml: &str,
    chart_name: &str,
    version: &str,
    repo_url: &str,
) -> Result<String> {
    let index: serde_yaml::Value =
        serde_yaml::from_str(index_yaml).map_err(|e| ReforgeError::Parse {
            file: "index.yaml".to_string(),
            reason: format!("Failed to parse index.yaml: {}", e),
        })?;

    let entries = index
        .get("entries")
        .and_then(|e| e.get(chart_name))
        .and_then(|e| e.as_sequence())
        .ok_or_else(|| ReforgeError::Registry {
            registry: repo_url.to_string(),
            message: format!("Chart '{}' not found in index", chart_name),
        })?;

    for entry in entries {
        let entry_version = entry
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if entry_version == version {
            // Prefer the first URL listed in the `urls` field.
            if let Some(url) = entry
                .get("urls")
                .and_then(|u| u.as_sequence())
                .and_then(|seq| seq.first())
                .and_then(|u| u.as_str())
            {
                // If it's a relative URL, make it absolute.
                if url.starts_with("http://") || url.starts_with("https://") {
                    return Ok(url.to_string());
                }
                return Ok(format!("{}/{}", repo_url.trim_end_matches('/'), url));
            }
        }
    }

    Err(ReforgeError::Registry {
        registry: repo_url.to_string(),
        message: format!("Version '{}' of chart '{}' not found in index", version, chart_name),
    })
}

async fn fetch_oci_digest(
    client: &reqwest::Client,
    image: &str,
    registry: Option<&str>,
    version: &str,
) -> Result<String> {
    let registry_host = registry.unwrap_or_else(|| {
        image.split('/').next().unwrap_or("registry-1.docker.io")
    });

    // Strip registry host from image path if present.
    let repo_path = if image.starts_with(registry_host) {
        image[registry_host.len()..].trim_start_matches('/')
    } else {
        image
    };

    let manifest_url = format!(
        "https://{}/v2/{}/manifests/{}",
        registry_host, repo_path, version
    );

    debug!("Fetching OCI manifest from {}", manifest_url);

    let resp = client
        .get(&manifest_url)
        .header(
            "Accept",
            "application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json",
        )
        .send()
        .await
        .map_err(|e| ReforgeError::Registry {
            registry: registry_host.to_string(),
            message: format!("Failed to fetch OCI manifest: {}", e),
        })?;

    // The `Docker-Content-Digest` header contains the digest.
    if let Some(digest) = resp.headers().get("Docker-Content-Digest") {
        return digest
            .to_str()
            .map(|s| s.to_string())
            .map_err(|_| ReforgeError::Registry {
                registry: registry_host.to_string(),
                message: "Invalid Docker-Content-Digest header".to_string(),
            });
    }

    // Fall back: compute SHA256 of the manifest body.
    let body = resp.bytes().await?;
    Ok(format!("sha256:{}", sha256_hex(&body)))
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOCK: &str = "\
dependencies:
- digest: sha256:aaaa
  name: ingress-nginx
  repository: https://kubernetes.github.io/ingress-nginx
  version: 4.8.3
- digest: sha256:bbbb
  name: redis
  repository: https://charts.bitnami.com/bitnami
  version: 18.4.0
digest: sha256:cccc
generated: \"2024-01-01T00:00:00.000000000Z\"
";

    #[test]
    fn test_parse_chart_lock() {
        let lock = parse_chart_lock(SAMPLE_LOCK).unwrap();
        assert_eq!(lock.dependencies.len(), 2);
        assert_eq!(lock.dependencies[0].name, "ingress-nginx");
        assert_eq!(lock.dependencies[0].version, "4.8.3");
        assert_eq!(lock.dependencies[1].name, "redis");
        assert_eq!(lock.dependencies[1].version, "18.4.0");
    }

    #[test]
    fn test_generate_chart_lock() {
        let deps = vec![
            ChartLockDependency {
                name: "nginx".to_string(),
                repository: "https://charts.example.com".to_string(),
                version: "1.2.3".to_string(),
            },
        ];
        let mut digests = HashMap::new();
        digests.insert("nginx".to_string(), "sha256:deadbeef".to_string());

        let output = generate_chart_lock(&deps, &digests);
        assert!(output.contains("name: nginx"));
        assert!(output.contains("version: 1.2.3"));
        assert!(output.contains("digest: sha256:deadbeef"));
        assert!(output.contains("repository: https://charts.example.com"));
    }

    #[test]
    fn test_update_chart_lock_version_and_digest() {
        let updated = update_chart_lock(SAMPLE_LOCK, "ingress-nginx", "4.9.0", "sha256:newdigest");
        assert!(updated.contains("version: 4.9.0"));
        assert!(updated.contains("digest: sha256:newdigest"));
        // The other dependency should be unchanged.
        assert!(updated.contains("version: 18.4.0"));
        assert!(updated.contains("digest: sha256:bbbb"));
    }

    #[test]
    fn test_update_chart_lock_second_dep() {
        let updated = update_chart_lock(SAMPLE_LOCK, "redis", "19.0.0", "sha256:redisdigest");
        assert!(updated.contains("version: 19.0.0"));
        assert!(updated.contains("digest: sha256:redisdigest"));
        // The first dependency should be unchanged.
        assert!(updated.contains("version: 4.8.3"));
        assert!(updated.contains("digest: sha256:aaaa"));
    }

    #[test]
    fn test_update_chart_lock_preserves_trailing_newline() {
        let updated = update_chart_lock(SAMPLE_LOCK, "redis", "19.0.0", "sha256:x");
        assert!(updated.ends_with('\n'));
    }

    #[test]
    fn test_update_chart_lock_unknown_dep_unchanged() {
        let updated = update_chart_lock(SAMPLE_LOCK, "nonexistent", "1.0.0", "sha256:x");
        // No changes expected when the dep doesn't exist.
        assert!(updated.contains("version: 4.8.3"));
        assert!(updated.contains("version: 18.4.0"));
    }
}
