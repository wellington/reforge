use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::debug;

use crate::error::{ReforgeError, Result};

/// Wraps the local `git` binary for async execution.
pub struct GitRepo {
    pub path: PathBuf,
}

impl GitRepo {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Run a git subcommand in the repo directory, returning stdout on success.
    async fn run(&self, args: &[&str]) -> Result<String> {
        self.run_in(&self.path, args).await
    }

    async fn run_in(&self, dir: &Path, args: &[&str]) -> Result<String> {
        debug!("git {}", args.join(" "));
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .await
            .map_err(|e| ReforgeError::Git(format!("Failed to spawn git: {}", e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            if stderr.contains("already exists") {
                if let Some(branch) = args
                    .windows(2)
                    .find(|w| w[0] == "-b")
                    .map(|w| w[1].to_string())
                {
                    return Err(ReforgeError::GitBranchExists { branch });
                }
            }

            Err(ReforgeError::GitCommand { exit_code, stderr })
        }
    }

    /// Clone a remote repository to a local path.
    pub async fn clone(url: &str, dest: &Path) -> Result<Self> {
        let output = Command::new("git")
            .args(["clone", url, dest.to_str().unwrap_or(".")])
            .output()
            .await
            .map_err(|e| ReforgeError::Git(format!("Failed to spawn git clone: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            return Err(ReforgeError::GitCommand { exit_code, stderr });
        }

        Ok(Self::new(dest))
    }

    /// Return the default branch (HEAD symbolic ref on origin, or current branch).
    pub async fn default_branch(&self) -> Result<String> {
        // Try to get the HEAD of origin
        let result = self
            .run(&["symbolic-ref", "refs/remotes/origin/HEAD"])
            .await;
        if let Ok(out) = result {
            let branch = out
                .trim()
                .trim_start_matches("refs/remotes/origin/")
                .to_string();
            if !branch.is_empty() {
                return Ok(branch);
            }
        }

        // Fall back to current branch
        self.current_branch().await
    }

    /// Return the currently checked-out branch name.
    pub async fn current_branch(&self) -> Result<String> {
        let out = self.run(&["rev-parse", "--abbrev-ref", "HEAD"]).await?;
        Ok(out.trim().to_string())
    }

    /// Checkout an existing branch.
    pub async fn checkout(&self, branch: &str) -> Result<()> {
        self.run(&["checkout", branch]).await?;
        Ok(())
    }

    /// Create a new branch from a base ref and check it out.
    pub async fn create_branch(&self, branch: &str, base: &str) -> Result<()> {
        self.run(&["checkout", "-b", branch, base]).await?;
        Ok(())
    }

    /// Check whether a branch exists locally.
    pub async fn branch_exists(&self, branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{}", branch)])
            .current_dir(&self.path)
            .output()
            .await
            .map_err(|e| ReforgeError::Git(format!("Failed to spawn git: {}", e)))?;
        Ok(output.status.success())
    }

    /// Stage a specific file and commit.
    pub async fn add_and_commit(&self, file_path: &str, message: &str) -> Result<String> {
        self.run(&["add", file_path]).await?;
        let out = self.run(&["commit", "-m", message]).await?;
        Ok(out.trim().to_string())
    }

    /// Stage all modified tracked files and commit.
    pub async fn commit_all(&self, message: &str) -> Result<String> {
        self.run(&["add", "-u"]).await?;
        let out = self.run(&["commit", "-m", message]).await?;
        Ok(out.trim().to_string())
    }

    /// Push the current branch to origin.
    pub async fn push(&self, branch: &str) -> Result<()> {
        self.run(&["push", "origin", branch]).await?;
        Ok(())
    }

    /// Get the short status of the working tree.
    pub async fn status(&self) -> Result<String> {
        self.run(&["status", "--short"]).await
    }

    /// Get the commit log as `hash subject` lines.
    pub async fn log(&self, max_count: usize) -> Result<Vec<LogEntry>> {
        let n = max_count.to_string();
        let out = self
            .run(&[
                "log",
                &format!("-{}", n),
                "--pretty=format:%H\t%s",
            ])
            .await?;

        let entries = out
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|line| {
                let mut parts = line.splitn(2, '\t');
                let hash = parts.next()?.to_string();
                let subject = parts.next().unwrap_or("").to_string();
                Some(LogEntry { hash, subject })
            })
            .collect();

        Ok(entries)
    }

    /// Walk the working tree and return all relative file paths.
    pub async fn list_files(&self) -> Result<Vec<String>> {
        let out = self.run(&["ls-files"]).await?;
        Ok(out
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    /// Read a file from the working tree at its current (possibly modified) state.
    pub async fn read_file(&self, relative_path: &str) -> Result<String> {
        let full_path = self.path.join(relative_path);
        tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ReforgeError::Git(format!("Cannot read {}: {}", full_path.display(), e)))
    }

    /// Write content to a file in the working tree.
    pub async fn write_file(&self, relative_path: &str, content: &str) -> Result<()> {
        let full_path = self.path.join(relative_path);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&full_path, content).await?;
        Ok(())
    }

    /// Verify that the directory is actually a git repository.
    pub async fn validate(&self) -> Result<()> {
        let output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(&self.path)
            .output()
            .await
            .map_err(|e| ReforgeError::Git(format!("Failed to spawn git: {}", e)))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(ReforgeError::GitRepoNotFound {
                path: self.path.display().to_string(),
            })
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub hash: String,
    pub subject: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn init_repo() -> (TempDir, GitRepo) {
        let dir = TempDir::new().unwrap();
        let path = dir.path();

        // Initialise a bare repo so tests don't need a remote
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(path)
            .output()
            .await
            .unwrap();

        // Initial commit so HEAD exists
        let readme = path.join("README.md");
        tokio::fs::write(&readme, "# test\n").await.unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(path)
            .output()
            .await
            .unwrap();

        let repo = GitRepo::new(path);
        (dir, repo)
    }

    #[tokio::test]
    async fn test_validate_valid_repo() {
        let (_dir, repo) = init_repo().await;
        assert!(repo.validate().await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_invalid_repo() {
        let dir = TempDir::new().unwrap();
        let repo = GitRepo::new(dir.path());
        assert!(matches!(
            repo.validate().await,
            Err(ReforgeError::GitRepoNotFound { .. })
        ));
    }

    #[tokio::test]
    async fn test_current_branch() {
        let (_dir, repo) = init_repo().await;
        let branch = repo.current_branch().await.unwrap();
        // git init creates either "main" or "master" depending on config
        assert!(!branch.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_switch_branch() {
        let (_dir, repo) = init_repo().await;
        let base = repo.current_branch().await.unwrap();
        repo.create_branch("feature/test", &base).await.unwrap();
        let branch = repo.current_branch().await.unwrap();
        assert_eq!(branch, "feature/test");
    }

    #[tokio::test]
    async fn test_branch_exists() {
        let (_dir, repo) = init_repo().await;
        let base = repo.current_branch().await.unwrap();
        assert!(!repo.branch_exists("new-branch").await.unwrap());
        repo.create_branch("new-branch", &base).await.unwrap();
        assert!(repo.branch_exists("new-branch").await.unwrap());
    }

    #[tokio::test]
    async fn test_write_read_file() {
        let (_dir, repo) = init_repo().await;
        repo.write_file("subdir/hello.txt", "hello world\n")
            .await
            .unwrap();
        let content = repo.read_file("subdir/hello.txt").await.unwrap();
        assert_eq!(content, "hello world\n");
    }

    #[tokio::test]
    async fn test_list_files() {
        let (_dir, repo) = init_repo().await;
        let files = repo.list_files().await.unwrap();
        assert!(files.contains(&"README.md".to_string()));
    }

    #[tokio::test]
    async fn test_add_and_commit() {
        let (_dir, repo) = init_repo().await;
        repo.write_file("new.txt", "content\n").await.unwrap();
        repo.run(&["add", "new.txt"]).await.unwrap();
        let out = repo
            .run(&["commit", "-m", "add new.txt"])
            .await
            .unwrap();
        assert!(!out.is_empty());

        let log = repo.log(2).await.unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].subject, "add new.txt");
    }

    #[tokio::test]
    async fn test_log_parsing() {
        let (_dir, repo) = init_repo().await;
        let log = repo.log(5).await.unwrap();
        assert!(!log.is_empty());
        assert_eq!(log[0].subject, "init");
        assert!(!log[0].hash.is_empty());
    }

    #[tokio::test]
    async fn test_status() {
        let (_dir, repo) = init_repo().await;
        let status = repo.status().await.unwrap();
        // Clean repo should have empty status
        assert!(status.trim().is_empty());
    }

    #[tokio::test]
    async fn test_git_command_error_has_exit_code() {
        let (_dir, repo) = init_repo().await;
        let err = repo.run(&["checkout", "nonexistent-branch-xyz"]).await;
        match err {
            Err(ReforgeError::GitCommand { exit_code, .. }) => {
                assert_ne!(exit_code, 0);
            }
            other => panic!("Expected GitCommand error, got {:?}", other),
        }
    }
}
