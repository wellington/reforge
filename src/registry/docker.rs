use base64::Engine;
use reqwest::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use serde::Deserialize;
use std::collections::HashMap;
use tracing::debug;

use crate::config::RegistryCredential;
use crate::error::{ReforgeError, Result};
use crate::manager::RegistrySource;
use crate::registry::{parse_version_lenient, RegistryClient, VersionInfo};

pub struct DockerRegistryClient {
    client: reqwest::Client,
    credentials: HashMap<String, RegistryCredential>,
}

#[derive(Debug, Deserialize)]
struct TagListResponse {
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
}

impl DockerRegistryClient {
    pub fn new(credentials: HashMap<String, RegistryCredential>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            credentials,
        }
    }

    fn resolve_registry_url(
        registry: &Option<String>,
        credentials: &HashMap<String, RegistryCredential>,
    ) -> (String, String) {
        match registry {
            Some(reg) => {
                let host = reg.split('/').next().unwrap_or(reg);
                if let Some(cred) = credentials.get(host) {
                    if let Some(base_url) = &cred.base_url {
                        return (base_url.trim_end_matches('/').to_string(), host.to_string());
                    }
                }
                (format!("https://{}", host), host.to_string())
            }
            None => {
                let default_host = "registry-1.docker.io";
                if let Some(cred) = credentials.get(default_host) {
                    if let Some(base_url) = &cred.base_url {
                        return (base_url.trim_end_matches('/').to_string(), default_host.to_string());
                    }
                }
                (
                    format!("https://{}", default_host),
                    default_host.to_string(),
                )
            }
        }
    }

    fn resolve_image_name(image: &str, registry: &Option<String>) -> String {
        if registry.is_none() && !image.contains('/') {
            format!("library/{}", image)
        } else if let Some(reg) = registry {
            let host = reg.split('/').next().unwrap_or(reg);
            if image.starts_with(host) {
                image.strip_prefix(host).unwrap_or(image).trim_start_matches('/').to_string()
            } else {
                if reg.contains('/') {
                    let path = reg.splitn(2, '/').nth(1).unwrap_or("");
                    if path.is_empty() {
                        image.to_string()
                    } else {
                        format!("{}/{}", path, image)
                    }
                } else {
                    image.to_string()
                }
            }
        } else {
            image.to_string()
        }
    }

    async fn authenticate(
        &self,
        www_authenticate: &str,
        registry_host: &str,
    ) -> Result<String> {
        let params = parse_www_authenticate(www_authenticate);
        let realm = params.get("realm").ok_or_else(|| {
            ReforgeError::Registry {
                registry: registry_host.to_string(),
                message: "Missing realm in WWW-Authenticate".to_string(),
            }
        })?;

        let mut url = realm.clone();
        let mut query_parts = Vec::new();
        if let Some(service) = params.get("service") {
            query_parts.push(format!("service={}", service));
        }
        if let Some(scope) = params.get("scope") {
            query_parts.push(format!("scope={}", scope));
        }
        if !query_parts.is_empty() {
            url = format!("{}?{}", url, query_parts.join("&"));
        }

        let mut req = self.client.get(&url);

        if let Some(cred) = self.credentials.get(registry_host) {
            if let (Some(username), Some(password)) =
                (&cred.username, cred.resolve_password())
            {
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", username, password));
                req = req.header(AUTHORIZATION, format!("Basic {}", encoded));
            }
        }

        let resp = req.send().await.map_err(|e| ReforgeError::Registry {
            registry: registry_host.to_string(),
            message: format!("Token request failed: {}", e),
        })?;

        if !resp.status().is_success() {
            return Err(ReforgeError::Registry {
                registry: registry_host.to_string(),
                message: format!("Token request returned {}", resp.status()),
            });
        }

        let token_resp: TokenResponse = resp.json().await.map_err(|e| {
            ReforgeError::Registry {
                registry: registry_host.to_string(),
                message: format!("Failed to parse token response: {}", e),
            }
        })?;

        token_resp
            .token
            .or(token_resp.access_token)
            .ok_or_else(|| ReforgeError::Registry {
                registry: registry_host.to_string(),
                message: "No token in response".to_string(),
            })
    }

    async fn fetch_tags(
        &self,
        registry_url: &str,
        registry_host: &str,
        image_name: &str,
    ) -> Result<Vec<String>> {
        let tags_url = format!("{}/v2/{}/tags/list", registry_url, image_name);
        debug!("Fetching tags from {}", tags_url);

        let mut req = self.client.get(&tags_url);

        // Pre-authenticate with Bearer token when the credential has a
        // password_env but no username (API-key-as-bearer pattern used by
        // Artifactory OCI registries).
        if let Some(cred) = self.credentials.get(registry_host) {
            if cred.username.is_none() {
                if let Some(password) = cred.resolve_password() {
                    req = req.header(AUTHORIZATION, format!("Bearer {}", password));
                }
            }
        }

        let resp = req.send().await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let www_auth = resp
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            let token = self.authenticate(&www_auth, registry_host).await?;

            let mut all_tags = Vec::new();
            let mut next_url = Some(tags_url);

            while let Some(url) = next_url {
                let resp = self
                    .client
                    .get(&url)
                    .header(AUTHORIZATION, format!("Bearer {}", token))
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    return Err(ReforgeError::Registry {
                        registry: registry_host.to_string(),
                        message: format!("Tag list request returned {}", resp.status()),
                    });
                }

                // Check for pagination Link header
                next_url = resp
                    .headers()
                    .get("link")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| parse_link_next(v, registry_url));

                let tag_list: TagListResponse = resp.json().await?;
                all_tags.extend(tag_list.tags);
            }

            Ok(all_tags)
        } else if resp.status().is_success() {
            let tag_list: TagListResponse = resp.json().await?;
            Ok(tag_list.tags)
        } else {
            Err(ReforgeError::Registry {
                registry: registry_host.to_string(),
                message: format!("Tag list request returned {}", resp.status()),
            })
        }
    }
}

#[async_trait::async_trait]
impl RegistryClient for DockerRegistryClient {
    async fn fetch_versions(&self, source: &RegistrySource) -> Result<Vec<VersionInfo>> {
        let (image, registry) = match source {
            RegistrySource::DockerRegistry { image, registry } => (image.clone(), registry.clone()),
            RegistrySource::OciHelmRegistry { image, registry } => {
                (image.clone(), registry.clone())
            }
            _ => {
                return Err(ReforgeError::Registry {
                    registry: "unknown".to_string(),
                    message: "DockerRegistryClient cannot handle HelmRepository source".to_string(),
                });
            }
        };

        let (registry_url, registry_host) = Self::resolve_registry_url(&registry, &self.credentials);
        let image_name = Self::resolve_image_name(&image, &registry);

        debug!(
            "Fetching versions for {} from {}",
            image_name, registry_url
        );

        let tags = self
            .fetch_tags(&registry_url, &registry_host, &image_name)
            .await?;

        let versions: Vec<VersionInfo> = tags
            .into_iter()
            .filter_map(|tag| {
                parse_version_lenient(&tag).map(|v| VersionInfo {
                    version: v,
                    original_tag: tag,
                })
            })
            .collect();

        debug!("Found {} parseable versions for {}", versions.len(), image);
        Ok(versions)
    }
}

fn parse_www_authenticate(header: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    // Strip "Bearer " prefix
    let content = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .unwrap_or(header);

    for part in content.split(',') {
        let part = part.trim();
        if let Some(eq_idx) = part.find('=') {
            let key = part[..eq_idx].trim().to_string();
            let val = part[eq_idx + 1..]
                .trim()
                .trim_matches('"')
                .to_string();
            params.insert(key, val);
        }
    }
    params
}

fn parse_link_next(header: &str, base_url: &str) -> Option<String> {
    for part in header.split(',') {
        let part = part.trim();
        if part.contains("rel=\"next\"") {
            if let Some(start) = part.find('<') {
                if let Some(end) = part.find('>') {
                    let url = &part[start + 1..end];
                    if url.starts_with("http") {
                        return Some(url.to_string());
                    } else {
                        return Some(format!("{}{}", base_url, url));
                    }
                }
            }
        }
    }
    None
}
