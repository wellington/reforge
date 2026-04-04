#[derive(thiserror::Error, Debug)]
pub enum ReforgeError {
    #[error("GitLab API error: {status} {message}")]
    GitLabApi { status: u16, message: String },

    #[error("Registry error for {registry}: {message}")]
    Registry { registry: String, message: String },

    #[error("Failed to parse {file}: {reason}")]
    Parse { file: String, reason: String },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Git command failed (exit {exit_code}): {stderr}")]
    GitCommand { exit_code: i32, stderr: String },

    #[error("Git repository not found at {path}")]
    GitRepoNotFound { path: String },

    #[error("Git branch '{branch}' already exists")]
    GitBranchExists { branch: String },

    #[error("Git operation failed: {0}")]
    Git(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Semver(#[from] semver::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, ReforgeError>;
