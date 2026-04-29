//! Error types for the reforge crate.
//!
//! This module defines [`ReforgeError`], a typed error enum covering all
//! failure modes in reforge: API errors, registry failures, configuration
//! issues, git operations, and I/O. The [`Result`] type alias is provided
//! for convenience.

/// All errors that can occur during reforge operations.
///
/// Each variant captures enough context to diagnose the failure. External
/// library errors (HTTP, YAML parsing, I/O, semver) are wrapped via
/// `#[from]` for ergonomic `?` usage.
#[derive(thiserror::Error, Debug)]
pub enum ReforgeError {
    /// GitLab API returned a non-success status code.
    #[error("GitLab API error: {status} {message}")]
    GitLabApi { status: u16, message: String },

    /// Failed to communicate with a container or Helm registry.
    #[error("Registry error for {registry}: {message}")]
    Registry { registry: String, message: String },

    /// Failed to parse a file (YAML, TOML, Dockerfile, etc.).
    #[error("Failed to parse {file}: {reason}")]
    Parse { file: String, reason: String },

    /// Invalid or missing configuration.
    #[error("Configuration error: {0}")]
    Config(String),

    /// A `git` subprocess exited with an error.
    #[error("Git command failed (exit {exit_code}): {stderr}")]
    GitCommand { exit_code: i32, stderr: String },

    /// The specified path is not a git repository.
    #[error("Git repository not found at {path}")]
    GitRepoNotFound { path: String },

    /// Attempted to create a branch that already exists.
    #[error("Git branch '{branch}' already exists")]
    GitBranchExists { branch: String },

    /// A git operation failed for a reason not covered by other variants.
    #[error("Git operation failed: {0}")]
    Git(String),

    /// HTTP request failed (wraps [`reqwest::Error`]).
    #[error(transparent)]
    Http(#[from] reqwest::Error),

    /// YAML parsing failed (wraps [`serde_yaml::Error`]).
    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),

    /// File I/O failed (wraps [`std::io::Error`]).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Semver parsing failed (wraps [`semver::Error`]).
    #[error(transparent)]
    Semver(#[from] semver::Error),
}

/// A specialized [`Result`] type for reforge operations.
pub type Result<T> = std::result::Result<T, ReforgeError>;
