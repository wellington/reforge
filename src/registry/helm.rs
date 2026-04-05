use serde::Deserialize;
use std::collections::HashMap;
use tracing::debug;

use crate::config::RegistryCredential;
use crate::error::{ReforgeError, Result};
use crate::manager::RegistrySource;
use crate::registry::docker::DockerRegistryClient;
use crate::registry::{parse_version_lenient, RegistryClient, VersionInfo};

pub struct HelmRegistryClient {
    client: reqwest::Client,
    docker_client: DockerRegistryClient,
    credentials: HashMap<String, RegistryCredential>,
}

#[derive(Debug, Deserialize)]
struct HelmIndex {
    entries: HashMap<String, Vec<HelmChartEntry>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HelmChartEntry {
    version: String,
    #[serde(default)]
    name: String,
}

impl HelmRegistryClient {
    pub fn new(credentials: HashMap<String, RegistryCredential>) -> crate::error::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        let docker_client = DockerRegistryClient::new(credentials.clone())?;

        Ok(Self {
            client,
            docker_client,
            credentials,
        })
    }

    async fn fetch_from_helm_repo(
        &self,
        repo_url: &str,
        chart_name: &str,
    ) -> Result<Vec<VersionInfo>> {
        let index_url = format!("{}/index.yaml", repo_url.trim_end_matches('/'));
        debug!("Fetching Helm index from {}", index_url);

        let mut req = self.client.get(&index_url);

        // Check if we have credentials for this repo host
        if let Ok(url) = url::Url::parse(&index_url) {
            if let Some(host) = url.host_str() {
                if let Some(cred) = self.credentials.get(host) {
                    if let (Some(username), Some(password)) =
                        (&cred.username, cred.resolve_password())
                    {
                        req = req.basic_auth(username, Some(password));
                    }
                }
            }
        }

        let resp = req.send().await.map_err(|e| ReforgeError::Registry {
            registry: repo_url.to_string(),
            message: format!("Failed to fetch index.yaml: {}", e),
        })?;

        if !resp.status().is_success() {
            return Err(ReforgeError::Registry {
                registry: repo_url.to_string(),
                message: format!("index.yaml returned {}", resp.status()),
            });
        }

        let body = resp.text().await?;
        let index: HelmIndex =
            serde_yaml::from_str(&body).map_err(|e| ReforgeError::Registry {
                registry: repo_url.to_string(),
                message: format!("Failed to parse index.yaml: {}", e),
            })?;

        let entries = index.entries.get(chart_name).ok_or_else(|| {
            ReforgeError::Registry {
                registry: repo_url.to_string(),
                message: format!("Chart '{}' not found in index", chart_name),
            }
        })?;

        let versions = entries
            .iter()
            .filter_map(|entry| {
                parse_version_lenient(&entry.version).map(|v| VersionInfo {
                    version: v,
                    original_tag: entry.version.clone(),
                })
            })
            .collect();

        Ok(versions)
    }
}

#[async_trait::async_trait]
impl RegistryClient for HelmRegistryClient {
    async fn fetch_versions(&self, source: &RegistrySource) -> Result<Vec<VersionInfo>> {
        match source {
            RegistrySource::HelmRepository {
                repo_url,
                chart_name,
            } => self.fetch_from_helm_repo(repo_url, chart_name).await,
            RegistrySource::OciHelmRegistry { .. } => {
                self.docker_client.fetch_versions(source).await
            }
            _ => Err(ReforgeError::Registry {
                registry: "unknown".to_string(),
                message: "HelmRegistryClient cannot handle DockerRegistry source".to_string(),
            }),
        }
    }
}
