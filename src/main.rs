mod config;
mod error;
mod manager;
mod orchestrator;
mod platform;
mod registry;
mod updater;
mod versioning;

use clap::Parser;
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::config::CliOverrides;
use crate::orchestrator::Orchestrator;

#[derive(Parser, Debug)]
#[command(name = "reforge", about = "Automated dependency updates for Helm charts and Dockerfiles")]
struct Cli {
    /// Path to config file
    #[arg(long, default_value = "reforge.toml")]
    config: PathBuf,

    /// GitLab project path (overrides config)
    #[arg(long)]
    repo: Option<String>,

    /// Log what would be done without creating MRs
    #[arg(long)]
    dry_run: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    /// GitLab API token (prefer env: REFORGE_TOKEN)
    #[arg(long, env = "REFORGE_TOKEN")]
    token: Option<String>,

    /// GitLab instance URL (prefer env: REFORGE_GITLAB_URL)
    #[arg(long, env = "REFORGE_GITLAB_URL")]
    gitlab_url: Option<String>,

    /// Output dry-run results as JSON
    #[arg(long)]
    json: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = EnvFilter::try_new(&cli.log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    info!("reforge v{}", env!("CARGO_PKG_VERSION"));

    let overrides = CliOverrides {
        token: cli.token,
        gitlab_url: cli.gitlab_url,
        repo: cli.repo,
    };

    let config = if cli.config.exists() {
        config::Config::load(&cli.config, overrides)?
    } else {
        info!("No config file found at {:?}, using CLI args and env vars", cli.config);
        config::Config::from_cli(overrides)?
    };

    let orchestrator = Orchestrator::new(config, cli.dry_run)?;
    orchestrator.run().await?;

    Ok(())
}
