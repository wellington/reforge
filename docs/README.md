# Reforge Documentation

Reforge is a Rust-native automated dependency updater for Helm charts and Dockerfiles, designed as a drop-in replacement for Renovate targeting self-managed GitLab.

## Contents

| Guide | Description |
|-------|-------------|
| [Getting Started](getting-started.md) | Install reforge and run your first scan |
| [Configuration Reference](configuration.md) | Complete `reforge.toml` reference |
| [Managers](managers.md) | Helm, Docker, and custom Regex managers |
| [Merge Requests](merge-requests.md) | MR creation, grouping, and automerge policies |
| [Advanced Features](advanced.md) | Scheduling, vulnerability awareness, changelogs, replacements, lock files |
| [Running in GitLab CI](gitlab-ci.md) | Scheduled pipeline setup |
| [Local Mode](local-mode.md) | Scanning a local git checkout without the GitLab API |

## What Reforge Does

1. **Scans** your GitLab projects for `Chart.yaml`, `values.yaml`, `Dockerfile`, and `docker-compose.yml` files.
2. **Resolves** current dependency versions against Docker Hub, OCI registries, and Helm repos.
3. **Proposes updates** by opening GitLab merge requests — one per dependency by default.
4. **Maintains** a Dependency Dashboard issue summarising all open and pending updates.

## Renovate Compatibility

Reforge accepts `RENOVATE_TOKEN` and `RENOVATE_GITLAB_URL` environment variables as fallbacks, making migration from Renovate straightforward.
