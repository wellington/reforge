# Local Mode

Local mode lets reforge scan a git repository on disk and commit updates directly to local branches — without any GitLab API calls. This is useful for:

- **Development and testing** — iterate on config without touching a live GitLab instance
- **Pre-flight checks** — see what reforge would do before deploying it to CI
- **Air-gapped environments** — run against a local clone where GitLab is unreachable

## How It Works

In local mode, reforge:

1. Scans the files in your local git checkout
2. Fetches versions from registries (network access still required for registry lookups)
3. Creates local git branches with the version updates committed
4. Writes the Dependency Dashboard to a local Markdown file (`DEPENDENCY_DASHBOARD.md` by default)

No GitLab token is needed. No MRs or issues are created.

## Quickstart

```bash
# Clone the repo you want to scan
git clone https://gitlab.example.com/my-group/my-repo
cd my-repo

# Run reforge in local mode
reforge --local-path . --dry-run
```

Remove `--dry-run` to actually create branches:

```bash
reforge --local-path .
```

After running, inspect the branches reforge created:

```bash
git branch | grep reforge/
```

## CLI Flag

```bash
reforge --local-path /path/to/repo
```

The path must be the root of a git repository (a directory containing a `.git` folder).

## Config File

You can also set the local path in `reforge.toml`:

```toml
local_path = "/path/to/repo"
```

Or combine both — `reforge.toml` for the scan configuration, CLI for the path:

```toml
# reforge.toml (no [gitlab] or [scan.projects] needed in local mode)

[managers]
enabled = ["helm", "docker"]

[versioning]
pin_strategy = "semver-minor"

[merge_request]
branch_prefix = "reforge/"

[dashboard]
local_path = "DEPENDENCY_DASHBOARD.md"
```

```bash
reforge --config reforge.toml --local-path /path/to/repo
```

## Dependency Dashboard in Local Mode

Instead of creating a GitLab issue, reforge writes the dashboard to a local Markdown file:

```toml
[dashboard]
local_path = "DEPENDENCY_DASHBOARD.md"
```

The file is written at the root of the scanned repository. You can commit it, open it in a Markdown viewer, or pipe it through any tool.

## Registry Credentials in Local Mode

Registry credentials work the same as in GitLab mode:

```toml
[registries."artifactory.example.com"]
password_env = "ARTIFACTORY_API_KEY"
```

```bash
export ARTIFACTORY_API_KEY=my-api-key
reforge --local-path .
```

## Dry Run with JSON Output

Combine local mode with `--dry-run --json` to get a machine-readable list of updates:

```bash
reforge --local-path . --dry-run --json | jq '.[] | {name, current, latest}'
```

## Differences from GitLab Mode

| Feature | Local Mode | GitLab Mode |
|---------|-----------|-------------|
| Branch creation | Local git branches | Remote branches via GitLab API |
| MR creation | Not available | GitLab merge requests |
| Dashboard | Local `.md` file | GitLab issue |
| Authentication | No token needed | Requires `REFORGE_TOKEN` |
| Rebase/stale MR handling | Not applicable | Supported |
| Automerge | Not applicable | Supported |

## Use in CI for Dry-Run Validation

Local mode is convenient for validating a new `reforge.toml` in CI before it runs in production:

```yaml
reforge:validate:
  stage: test
  image: ghcr.io/wellington/reforge:latest
  before_script:
    - git clone $CI_REPOSITORY_URL /tmp/repo
  script:
    - reforge --config reforge.toml --local-path /tmp/repo --dry-run
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
      changes:
        - reforge.toml
```
