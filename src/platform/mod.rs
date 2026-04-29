//! Platform abstractions for file and git operations.
//!
//! This module provides the [`FileSource`] trait which abstracts over:
//! - [`GitLabSource`] — operates via the GitLab REST API
//! - [`LocalGitSource`] — operates on a local git checkout
//!
//! This abstraction allows the orchestrator to work identically whether
//! running against a remote GitLab project or a local repository.

pub mod git;
pub mod gitlab;

use async_trait::async_trait;
use std::path::PathBuf;

use crate::error::Result;

/// A file entry from a repository tree listing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileEntry {
    /// Relative path from repository root.
    pub path: String,
}

/// Abstracts file and git operations over different backends.
///
/// This trait allows reforge to work with both:
/// - Remote GitLab projects via the REST API
/// - Local git checkouts via shell commands
///
/// Implementations handle authentication, API pagination, and the
/// differences between remote and local file operations.
#[async_trait]
pub trait FileSource: Send + Sync {
    /// Returns the default branch name (e.g., "main" or "master").
    async fn default_branch(&self) -> Result<String>;

    /// Lists all files in the repository tree.
    ///
    /// Returns only blob entries (files), not trees (directories).
    async fn list_files(&self, branch: &str) -> Result<Vec<FileEntry>>;

    /// Reads the content of a file at the specified path and branch.
    async fn get_file(&self, path: &str, branch: &str) -> Result<String>;

    /// Creates a new branch from a base reference.
    async fn create_branch(&self, branch: &str, base: &str) -> Result<()>;

    /// Checks whether a branch exists.
    async fn branch_exists(&self, branch: &str) -> Result<bool>;

    /// Writes file content and commits it to the specified branch.
    ///
    /// Returns an identifier for the commit (format varies by backend).
    async fn commit_file(
        &self,
        branch: &str,
        file_path: &str,
        content: &str,
        message: &str,
    ) -> Result<String>;
}

// ── GitLab API source ────────────────────────────────────────────────────────

use crate::platform::gitlab::GitLabClient;

pub struct GitLabSource {
    pub client: GitLabClient,
    pub project: String,
}

#[async_trait]
impl FileSource for GitLabSource {
    async fn default_branch(&self) -> Result<String> {
        self.client.get_default_branch(&self.project).await
    }

    async fn list_files(&self, branch: &str) -> Result<Vec<FileEntry>> {
        let tree = self
            .client
            .list_tree(&self.project, branch, None, true)
            .await?;
        Ok(tree
            .into_iter()
            .filter(|e| e.entry_type == "blob")
            .map(|e| FileEntry { path: e.path })
            .collect())
    }

    async fn get_file(&self, path: &str, branch: &str) -> Result<String> {
        self.client.get_file(&self.project, path, branch).await
    }

    async fn create_branch(&self, branch: &str, base: &str) -> Result<()> {
        self.client
            .create_branch(&self.project, branch, base)
            .await
    }

    async fn branch_exists(&self, branch: &str) -> Result<bool> {
        self.client.branch_exists(&self.project, branch).await
    }

    async fn commit_file(
        &self,
        branch: &str,
        file_path: &str,
        content: &str,
        message: &str,
    ) -> Result<String> {
        use crate::platform::gitlab::{CommitAction, CommitActionKind};
        self.client
            .commit_files(
                &self.project,
                branch,
                message,
                vec![CommitAction {
                    action: CommitActionKind::Update,
                    file_path: file_path.to_string(),
                    content: content.to_string(),
                }],
            )
            .await?;
        Ok(format!("gitlab:{}/{}", self.project, branch))
    }
}

// ── Local git source ─────────────────────────────────────────────────────────

use crate::platform::git::GitRepo;

pub struct LocalGitSource {
    pub repo: GitRepo,
}

impl LocalGitSource {
    pub fn new(path: PathBuf) -> Self {
        Self {
            repo: GitRepo::new(path),
        }
    }
}

#[async_trait]
impl FileSource for LocalGitSource {
    async fn default_branch(&self) -> Result<String> {
        self.repo.default_branch().await
    }

    async fn list_files(&self, _branch: &str) -> Result<Vec<FileEntry>> {
        // We operate on the currently checked-out working tree.
        let paths = self.repo.list_files().await?;
        Ok(paths.into_iter().map(|p| FileEntry { path: p }).collect())
    }

    async fn get_file(&self, path: &str, _branch: &str) -> Result<String> {
        self.repo.read_file(path).await
    }

    async fn create_branch(&self, branch: &str, base: &str) -> Result<()> {
        self.repo.create_branch(branch, base).await
    }

    async fn branch_exists(&self, branch: &str) -> Result<bool> {
        self.repo.branch_exists(branch).await
    }

    async fn commit_file(
        &self,
        _branch: &str,
        file_path: &str,
        content: &str,
        message: &str,
    ) -> Result<String> {
        self.repo.write_file(file_path, content).await?;
        self.repo.add_and_commit(file_path, message).await
    }
}
