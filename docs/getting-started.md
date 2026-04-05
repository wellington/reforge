# Getting Started

## Prerequisites

- Rust toolchain (stable, 1.75+) — install via [rustup](https://rustup.rs)
- A GitLab personal access token with `api` scope (or `read_api` for dry-run only)
- Network access to your GitLab instance and relevant container/Helm registries

## Installation

### From source

```bash
git clone https://github.com/procore/reforge
cd reforge
cargo build --release
# Binary is at target/release/reforge
```

### Docker image

```bash
docker pull ghcr.io/procore/reforge:latest
```

## Quickstart

### 1. Create a minimal config

```toml
# reforge.toml
[gitlab]
url = "https://gitlab.example.com"

[scan]
projects = ["my-group/my-repo"]
```

### 2. Export your token

```bash
export REFORGE_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx
```

### 3. Run a dry run

```bash
reforge --dry-run
```

This prints what updates would be created without touching your GitLab instance.

### 4. Run for real

```bash
reforge
```

Reforge will:
- Open one merge request per dependency that has a newer version available
- Create or update a **Dependency Dashboard** issue listing all pending updates
- Skip any dependency that already has an open reforge MR

## CLI Reference

```
reforge [OPTIONS]

Options:
  --config <PATH>        Path to config file [default: reforge.toml]
  --repo <REPO>          GitLab project path (overrides config)
  --dry-run              Print proposed changes without creating MRs
  --log-level <LEVEL>    Log verbosity: error, warn, info, debug, trace [default: info]
  --token <TOKEN>        GitLab API token (prefer env: REFORGE_TOKEN)
  --gitlab-url <URL>     GitLab instance URL (prefer env: REFORGE_GITLAB_URL)
  --json                 Output dry-run results as JSON
  --local-path <PATH>    Path to a local git checkout (enables local mode)
  --no-dashboard         Disable the Dependency Dashboard issue
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `REFORGE_TOKEN` | GitLab personal access token |
| `REFORGE_GITLAB_URL` | GitLab instance base URL |
| `RENOVATE_TOKEN` | Accepted as a fallback for `REFORGE_TOKEN` |
| `RENOVATE_GITLAB_URL` | Accepted as a fallback for `REFORGE_GITLAB_URL` |
| `GITHUB_TOKEN` | GitHub token for fetching release changelogs |

## Next Steps

- Read the [Configuration Reference](configuration.md) for the full set of options.
- Learn how [Managers](managers.md) detect dependencies in your files.
- Set up a [GitLab CI scheduled pipeline](gitlab-ci.md) to run reforge automatically.
