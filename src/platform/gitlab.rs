use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::{ReforgeError, Result};

const PRIVATE_TOKEN: &str = "PRIVATE-TOKEN";
const MAX_RETRIES: u32 = 3;

pub struct GitLabClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct TreeEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub mode: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitActionKind {
    Create,
    Update,
    Delete,
    Move,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitAction {
    pub action: CommitActionKind,
    pub file_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateMrParams {
    pub source_branch: String,
    pub target_branch: String,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assignee_ids: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_when_pipeline_succeeds: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[allow(dead_code)]
pub struct UpdateMrParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_event: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MergeRequest {
    pub iid: u64,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub state: String,
    pub web_url: String,
}

/// Extended MR view that includes conflict and staleness fields.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MrDetail {
    pub iid: u64,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub state: String,
    pub web_url: String,
    #[serde(default)]
    pub has_conflicts: bool,
    pub diverged_commits_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectInfo {
    default_branch: String,
}

#[derive(Debug, Clone, Deserialize)]
struct FileResponse {
    content: String,
    encoding: String,
}

#[allow(dead_code)]
impl GitLabClient {
    pub fn new(url: &str, token: &str) -> Result<Self> {
        Self::with_options(url, token, false)
    }

    pub fn with_options(url: &str, token: &str, insecure: bool) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .danger_accept_invalid_certs(insecure)
            .build()?;

        Ok(Self {
            client,
            base_url: url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v4{}", self.base_url, path)
    }

    fn encode_project(project: &str) -> String {
        project.replace('/', "%2F")
    }

    async fn request_with_retry(
        &self,
        method: reqwest::Method,
        url: &str,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(self
            .client
            .request(method, url)
            .header(PRIVATE_TOKEN, &self.token))
    }

    async fn send_with_retry(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let mut last_err = None;

        for attempt in 0..MAX_RETRIES {
            let req = request
                .try_clone()
                .ok_or_else(|| ReforgeError::Config("Request body not cloneable".into()))?;

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }

                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        if let Some(retry_after) = resp.headers().get("retry-after") {
                            if let Ok(secs) = retry_after.to_str().unwrap_or("5").parse::<u64>() {
                                warn!("Rate limited, waiting {}s", secs);
                                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                                continue;
                            }
                        }
                    }

                    if status.is_server_error() && attempt < MAX_RETRIES - 1 {
                        let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt));
                        warn!("Server error {}, retrying in {:?}", status, delay);
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    let message = resp.text().await.unwrap_or_default();
                    return Err(ReforgeError::GitLabApi {
                        status: status.as_u16(),
                        message,
                    });
                }
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt));
                        warn!("Request error: {}, retrying in {:?}", e, delay);
                        tokio::time::sleep(delay).await;
                        last_err = Some(e);
                    } else {
                        return Err(e.into());
                    }
                }
            }
        }

        Err(last_err.map(ReforgeError::Http).unwrap_or_else(|| {
            ReforgeError::Config("Max retries exceeded".into())
        }))
    }

    pub async fn get_default_branch(&self, project: &str) -> Result<String> {
        let url = self.api_url(&format!(
            "/projects/{}",
            Self::encode_project(project)
        ));
        let req = self
            .client
            .get(&url)
            .header(PRIVATE_TOKEN, &self.token);

        let resp = self.send_with_retry(req).await?;
        let info: ProjectInfo = resp.json().await?;
        Ok(info.default_branch)
    }

    pub async fn list_tree(
        &self,
        project: &str,
        ref_: &str,
        path: Option<&str>,
        recursive: bool,
    ) -> Result<Vec<TreeEntry>> {
        let mut entries = Vec::new();
        let mut page = 1u32;
        let per_page = 100;

        loop {
            let mut url = format!(
                "{}/api/v4/projects/{}/repository/tree?ref={}&per_page={}&page={}",
                self.base_url,
                Self::encode_project(project),
                ref_,
                per_page,
                page,
            );
            if let Some(p) = path {
                url.push_str(&format!("&path={}", p));
            }
            if recursive {
                url.push_str("&recursive=true");
            }

            let req = self.client.get(&url).header(PRIVATE_TOKEN, &self.token);
            let resp = self.send_with_retry(req).await?;

            let next_page = resp
                .headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u32>().ok());

            let batch: Vec<TreeEntry> = resp.json().await?;
            let batch_len = batch.len();
            entries.extend(batch);

            match next_page {
                Some(np) if batch_len == per_page as usize => page = np,
                _ => break,
            }
        }

        Ok(entries)
    }

    pub async fn get_file(
        &self,
        project: &str,
        path: &str,
        ref_: &str,
    ) -> Result<String> {
        let encoded_path = urlencoding::encode(path);
        let url = self.api_url(&format!(
            "/projects/{}/repository/files/{}?ref={}",
            Self::encode_project(project),
            encoded_path,
            ref_,
        ));

        let req = self.client.get(&url).header(PRIVATE_TOKEN, &self.token);
        let resp = self.send_with_retry(req).await?;
        let file_resp: FileResponse = resp.json().await?;

        if file_resp.encoding == "base64" {
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(file_resp.content.replace('\n', ""))
                .map_err(|e| ReforgeError::Parse {
                    file: path.to_string(),
                    reason: format!("Base64 decode failed: {}", e),
                })?;
            String::from_utf8(decoded).map_err(|e| ReforgeError::Parse {
                file: path.to_string(),
                reason: format!("UTF-8 decode failed: {}", e),
            })
        } else {
            Ok(file_resp.content)
        }
    }

    pub async fn create_branch(
        &self,
        project: &str,
        branch: &str,
        ref_: &str,
    ) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/repository/branches",
            Self::encode_project(project),
        ));

        let req = self
            .client
            .post(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&serde_json::json!({
                "branch": branch,
                "ref": ref_,
            }));

        self.send_with_retry(req).await?;
        debug!("Created branch {}", branch);
        Ok(())
    }

    pub async fn delete_branch(&self, project: &str, branch: &str) -> Result<()> {
        let encoded_branch = urlencoding::encode(branch);
        let url = self.api_url(&format!(
            "/projects/{}/repository/branches/{}",
            Self::encode_project(project),
            encoded_branch,
        ));

        let req = self
            .client
            .delete(&url)
            .header(PRIVATE_TOKEN, &self.token);

        self.send_with_retry(req).await?;
        debug!("Deleted branch {}", branch);
        Ok(())
    }

    pub async fn branch_exists(&self, project: &str, branch: &str) -> Result<bool> {
        let encoded_branch = urlencoding::encode(branch);
        let url = self.api_url(&format!(
            "/projects/{}/repository/branches/{}",
            Self::encode_project(project),
            encoded_branch,
        ));

        let req = self
            .client
            .get(&url)
            .header(PRIVATE_TOKEN, &self.token);

        let resp = req.send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn commit_files(
        &self,
        project: &str,
        branch: &str,
        message: &str,
        actions: Vec<CommitAction>,
    ) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/repository/commits",
            Self::encode_project(project),
        ));

        let req = self
            .client
            .post(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&serde_json::json!({
                "branch": branch,
                "commit_message": message,
                "actions": actions,
            }));

        self.send_with_retry(req).await?;
        debug!("Committed {} files to {}", actions.len(), branch);
        Ok(())
    }

    pub async fn create_mr(
        &self,
        project: &str,
        params: CreateMrParams,
    ) -> Result<MergeRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests",
            Self::encode_project(project),
        ));

        let mut body = serde_json::json!({
            "source_branch": params.source_branch,
            "target_branch": params.target_branch,
            "title": params.title,
            "description": params.description,
        });

        if !params.labels.is_empty() {
            body["labels"] = serde_json::Value::String(params.labels.join(","));
        }
        if !params.assignee_ids.is_empty() {
            body["assignee_ids"] = serde_json::json!(params.assignee_ids);
        }
        if let Some(auto_merge) = params.merge_when_pipeline_succeeds {
            body["merge_when_pipeline_succeeds"] = serde_json::Value::Bool(auto_merge);
        }

        let req = self
            .client
            .post(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&body);

        let resp = self.send_with_retry(req).await?;
        let mr: MergeRequest = resp.json().await?;
        debug!("Created MR !{}: {}", mr.iid, mr.title);
        Ok(mr)
    }

    pub async fn list_open_mrs(
        &self,
        project: &str,
        source_branch_prefix: Option<&str>,
    ) -> Result<Vec<MergeRequest>> {
        let mut mrs = Vec::new();
        let mut page = 1u32;
        let per_page = 100;

        loop {
            let url = self.api_url(&format!(
                "/projects/{}/merge_requests?state=opened&per_page={}&page={}",
                Self::encode_project(project),
                per_page,
                page,
            ));

            let req = self.client.get(&url).header(PRIVATE_TOKEN, &self.token);
            let resp = self.send_with_retry(req).await?;

            let next_page = resp
                .headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u32>().ok());

            let batch: Vec<MergeRequest> = resp.json().await?;
            let batch_len = batch.len();
            mrs.extend(batch);

            match next_page {
                Some(np) if batch_len == per_page as usize => page = np,
                _ => break,
            }
        }

        if let Some(prefix) = source_branch_prefix {
            mrs.retain(|mr| mr.source_branch.starts_with(prefix));
        }

        Ok(mrs)
    }

    pub async fn update_mr(
        &self,
        project: &str,
        mr_iid: u64,
        params: UpdateMrParams,
    ) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}",
            Self::encode_project(project),
            mr_iid,
        ));

        let req = self
            .client
            .put(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&params);

        self.send_with_retry(req).await?;
        debug!("Updated MR !{}", mr_iid);
        Ok(())
    }

    pub async fn close_mr(&self, project: &str, mr_iid: u64) -> Result<()> {
        self.update_mr(
            project,
            mr_iid,
            UpdateMrParams {
                state_event: Some("close".to_string()),
                ..Default::default()
            },
        )
        .await
    }

    /// Set merge-when-pipeline-succeeds on an existing MR.
    pub async fn merge_mr(
        &self,
        project: &str,
        mr_iid: u64,
        merge_when_pipeline_succeeds: bool,
    ) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/merge",
            Self::encode_project(project),
            mr_iid,
        ));

        let req = self
            .client
            .put(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&serde_json::json!({
                "merge_when_pipeline_succeeds": merge_when_pipeline_succeeds,
            }));

        self.send_with_retry(req).await?;
        debug!(
            "Set merge_when_pipeline_succeeds={} on MR !{}",
            merge_when_pipeline_succeeds, mr_iid
        );
        Ok(())
    }

    /// Immediately accept (merge) an MR without waiting for the pipeline.
    pub async fn accept_mr(&self, project: &str, mr_iid: u64) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/merge",
            Self::encode_project(project),
            mr_iid,
        ));

        let req = self
            .client
            .put(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&serde_json::json!({}));

        self.send_with_retry(req).await?;
        debug!("Accepted MR !{}", mr_iid);
        Ok(())
    }

    /// Fetch the detailed view of a single MR (includes has_conflicts and diverged_commits_count).
    pub async fn get_mr_detail(&self, project: &str, mr_iid: u64) -> Result<MrDetail> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}",
            Self::encode_project(project),
            mr_iid,
        ));

        let req = self.client.get(&url).header(PRIVATE_TOKEN, &self.token);
        let resp = self.send_with_retry(req).await?;
        let detail: MrDetail = resp.json().await?;
        Ok(detail)
    }

    /// Trigger a GitLab-side rebase of the MR's source branch onto its target branch.
    /// Uses PUT /projects/:id/merge_requests/:iid/rebase
    pub async fn rebase_mr(&self, project: &str, mr_iid: u64) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/rebase",
            Self::encode_project(project),
            mr_iid,
        ));

        let req = self
            .client
            .put(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&serde_json::json!({}));

        self.send_with_retry(req).await?;
        debug!("Triggered rebase for MR !{}", mr_iid);
        Ok(())
    }

    pub async fn list_issues(
        &self,
        project: &str,
        search_title: Option<&str>,
        state: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let mut issues = Vec::new();
        let mut page = 1u32;
        let per_page = 100;

        loop {
            let mut url = format!(
                "{}/api/v4/projects/{}/issues?per_page={}&page={}",
                self.base_url,
                Self::encode_project(project),
                per_page,
                page,
            );
            if let Some(s) = state {
                url.push_str(&format!("&state={}", s));
            }
            if let Some(title) = search_title {
                url.push_str(&format!("&search={}", urlencoding::encode(title)));
            }

            let req = self.client.get(&url).header(PRIVATE_TOKEN, &self.token);
            let resp = self.send_with_retry(req).await?;

            let next_page = resp
                .headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u32>().ok());

            let batch: Vec<Issue> = resp.json().await?;
            let batch_len = batch.len();
            issues.extend(batch);

            match next_page {
                Some(np) if batch_len == per_page as usize => page = np,
                _ => break,
            }
        }

        Ok(issues)
    }

    pub async fn create_issue(
        &self,
        project: &str,
        title: &str,
        description: &str,
        labels: &[String],
    ) -> Result<Issue> {
        let url = self.api_url(&format!(
            "/projects/{}/issues",
            Self::encode_project(project),
        ));

        let mut body = serde_json::json!({
            "title": title,
            "description": description,
        });

        if !labels.is_empty() {
            body["labels"] = serde_json::Value::String(labels.join(","));
        }

        let req = self
            .client
            .post(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&body);

        let resp = self.send_with_retry(req).await?;
        let issue: Issue = resp.json().await?;
        debug!("Created issue #{}: {}", issue.iid, issue.title);
        Ok(issue)
    }

    pub async fn update_issue(
        &self,
        project: &str,
        issue_iid: u64,
        description: &str,
    ) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}",
            Self::encode_project(project),
            issue_iid,
        ));

        let req = self
            .client
            .put(&url)
            .header(PRIVATE_TOKEN, &self.token)
            .json(&serde_json::json!({ "description": description }));

        self.send_with_retry(req).await?;
        debug!("Updated issue #{}", issue_iid);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Issue {
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub web_url: String,
    pub state: String,
}

mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        result
    }
}
