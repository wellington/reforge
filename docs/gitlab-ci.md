# Running Reforge in GitLab CI

The recommended way to run reforge continuously is as a **scheduled GitLab CI pipeline**. This page covers setting up the pipeline, configuring secrets, and tuning the schedule.

## Overview

The pipeline has two stages:

1. **build** — compile the reforge binary (or pull the pre-built Docker image)
2. **scan** — run `reforge --config reforge.toml` against your configured projects

## Setting Up CI/CD Variables

In your GitLab project go to **Settings → CI/CD → Variables** and add:

| Variable | Description | Masked |
|----------|-------------|--------|
| `REFORGE_TOKEN` | GitLab personal access token with `api` scope | yes |
| `ARTIFACTORY_API_KEY` | Artifactory API key (if using private OCI registries) | yes |
| `GITHUB_TOKEN` | GitHub PAT for changelog fetching (optional) | yes |

> Use a [GitLab bot user](https://docs.gitlab.com/ee/user/profile/service_accounts.html) rather than a personal token in production.

## Pipeline Definition

Create `.gitlab-ci.yml` in the repo that houses your `reforge.toml`:

```yaml
# .gitlab-ci.yml

stages:
  - scan

variables:
  REFORGE_IMAGE: ghcr.io/procore/reforge:latest

reforge:scan:
  stage: scan
  image: $REFORGE_IMAGE
  script:
    - reforge --config reforge.toml
  rules:
    # Run on schedule only (not on every push)
    - if: $CI_PIPELINE_SOURCE == "schedule"
    # Allow manual trigger from the UI
    - if: $CI_PIPELINE_SOURCE == "web"
      when: manual
  # Surface the exit code but don't fail the pipeline if reforge finds no updates
  allow_failure: false
```

## Configuring a Schedule

Go to **CI/CD → Schedules → New Schedule** and set:

- **Description:** `Reforge dependency scan`
- **Interval:** `0 */6 * * *` (every 6 hours) or `0 9 * * 1-5` (weekdays at 09:00 UTC)
- **Branch:** `main`

## Reforge Configuration

Check a `reforge.toml` into the same repo as the pipeline:

```toml
[gitlab]
url = "https://gitlab.example.com"
# REFORGE_TOKEN is read from the CI/CD variable automatically

[scan]
projects = [
  "my-group/service-a",
  "my-group/service-b",
]

[managers]
enabled = ["helm", "docker"]

[versioning]
pin_strategy = "semver-minor"

[merge_request]
branch_prefix    = "reforge/"
labels           = ["reforge", "automated"]
max_open_mrs     = 20
rebase_enabled   = true
stale_mr_strategy = "rebase"

[registries."artifactory.example.com"]
password_env = "ARTIFACTORY_API_KEY"

[dashboard]
enabled = true

[changelog]
enabled = true
```

## Building from Source in CI

If you prefer to build reforge from source in CI rather than pulling the pre-built image:

```yaml
stages:
  - build
  - scan

reforge:build:
  stage: build
  image: rust:1-slim-bookworm
  before_script:
    - apt-get update && apt-get install -y pkg-config
  script:
    - cargo build --release --bin reforge
  artifacts:
    paths:
      - target/release/reforge
    expire_in: 1 day
  cache:
    key: cargo-$CI_COMMIT_REF_SLUG
    paths:
      - target/
      - $CARGO_HOME/registry/

reforge:scan:
  stage: scan
  image: debian:bookworm-slim
  before_script:
    - apt-get update && apt-get install -y ca-certificates git
  script:
    - ./target/release/reforge --config reforge.toml
  needs: ["reforge:build"]
  rules:
    - if: $CI_PIPELINE_SOURCE == "schedule"
    - if: $CI_PIPELINE_SOURCE == "web"
      when: manual
```

## Dry-Run Pipeline

Add a dry-run job that runs on every MR to validate your `reforge.toml` without touching any projects:

```yaml
reforge:dry-run:
  stage: scan
  image: $REFORGE_IMAGE
  script:
    - reforge --config reforge.toml --dry-run --log-level debug
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
```

## Token Permissions

The GitLab token used by reforge needs:

| Permission | Why |
|------------|-----|
| `api` | Read/write: list files, create branches, open MRs, manage issues |
| `read_repository` | (included in `api`) Read file contents from scanned projects |

For a read-only dry-run setup, `read_api` is sufficient but reforge will error when it attempts to write.

## Troubleshooting

### Registry authentication errors

Check that `ARTIFACTORY_API_KEY` (or your registry env var) is set in **Settings → CI/CD → Variables** and is **not** protected if the pipeline runs on unprotected branches.

### TLS errors against internal GitLab

Add `insecure = true` to `[gitlab]` in `reforge.toml` if your instance uses a self-signed certificate.

### Rate limiting

Lower `max_open_mrs` and space out schedule runs if your team is being overwhelmed by MR volume.
