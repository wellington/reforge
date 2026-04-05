# Configuration Reference

Reforge is configured via a TOML file (default: `reforge.toml`). Every section is optional — you can drive everything from CLI flags and environment variables.

Config is resolved in this priority order (highest wins):

1. CLI flags
2. Environment variables
3. `reforge.toml` values
4. Defaults

## Full Example

```toml
[gitlab]
url     = "https://gitlab.example.com"
# token loaded from REFORGE_TOKEN env var
insecure = false   # set true for self-signed TLS certs

[scan]
projects = [
  "my-group/my-repo",
  "my-group/another-repo",
]

[managers]
enabled = ["helm", "docker"]   # default; add "regex" implicitly via regex_managers

[versioning]
pin_strategy = "semver-minor"  # "semver-patch" | "semver-minor" | "semver-major"

[merge_request]
branch_prefix   = "reforge/"
labels          = ["reforge", "automated"]
assignees       = []           # GitLab user IDs
auto_merge      = false
max_open_mrs    = 10           # optional cap on concurrent open MRs
rebase_enabled  = true
stale_mr_strategy = "rebase"  # "rebase" | "recreate" | "ignore"

[dashboard]
enabled    = true
labels     = ["reforge", "dependency-dashboard"]
local_path = "DEPENDENCY_DASHBOARD.md"  # used in local mode

[changelog]
enabled    = true
max_length = 2000              # characters before truncation

[vulnerability]
enabled         = false
security_labels = ["security"]
priority_boost  = true         # security updates bypass rate limits

[lockfile]
enabled     = true
helm_binary = "/usr/local/bin/helm"   # optional; omit to use built-in digest computation

[replacement]
enabled    = true
rules_file = "replacements.toml"      # optional path to custom replacement rules
warn_only  = false                    # true = log warning instead of opening MR

# Registry credentials
[registries."registry.example.com"]
username     = "deploy-token"
password_env = "REFORGE_REGISTRY_EXAMPLE_PASSWORD"

[registries."artifactory.example.com"]
username     = "api-key-user"
password_env = "REFORGE_ARTIFACTORY_PASSWORD"
base_url     = "https://artifactory.example.com"  # override registry URL

# Custom regex managers (see managers.md)
[[regex_managers]]
name          = "helm-chart-version"
file_patterns = ["*.yaml", "*.yml"]
match_pattern = 'helmChart:\s+"(?P<depName>[^"]+)"\s+helmVersion:\s+"(?P<currentValue>[^"]+)"'
datasource    = "helm-repo"
registry_url  = "https://charts.example.com"
```

## Sections

### `[gitlab]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `url` | string | `https://gitlab.com` | GitLab instance base URL |
| `token` | string | — | API token (prefer `REFORGE_TOKEN` env var) |
| `insecure` | bool | `false` | Skip TLS certificate verification |

### `[scan]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `projects` | string[] | `[]` | GitLab project paths to scan (e.g. `"group/repo"`) |

Can be overridden per-run with `--repo group/repo`.

### `[managers]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | string[] | `["helm", "docker"]` | Active managers |

Valid values: `helm`, `docker`. Custom regex managers are always active when `[[regex_managers]]` entries are present.

### `[versioning]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `pin_strategy` | string | `semver-minor` | Version ceiling: `semver-patch`, `semver-minor`, `semver-major` |

- `semver-patch` — only propose patch updates (e.g. `1.2.3 → 1.2.9`)
- `semver-minor` — propose patch and minor updates (e.g. `1.2.3 → 1.9.0`)
- `semver-major` — propose any update including major bumps

### `[merge_request]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `branch_prefix` | string | `reforge/` | Prefix for update branches |
| `labels` | string[] | `["reforge", "automated"]` | Labels applied to every MR |
| `assignees` | integer[] | `[]` | GitLab user IDs to assign MRs to |
| `auto_merge` | bool | `false` | Set "merge when pipeline succeeds" on all MRs |
| `max_open_mrs` | integer | unlimited | Cap on concurrent open reforge MRs |
| `schedule_window` | object | — | Time window for creating new MRs (see [Advanced](advanced.md)) |
| `rebase_enabled` | bool | `true` | Check for stale MRs and apply the stale strategy |
| `stale_mr_strategy` | string | `rebase` | How to handle stale MRs: `rebase`, `recreate`, `ignore` |
| `automerge_policies` | object[] | `[]` | Fine-grained automerge rules (see [Merge Requests](merge-requests.md)) |
| `grouping_rules` | object[] | `[]` | Group candidates into combined MRs (see [Merge Requests](merge-requests.md)) |

### `[dashboard]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Create/update a Dependency Dashboard GitLab issue |
| `labels` | string[] | `["reforge", "dependency-dashboard"]` | Labels for the dashboard issue |
| `local_path` | string | `DEPENDENCY_DASHBOARD.md` | Output file path in local mode |

### `[changelog]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Fetch and embed GitHub release notes in MR descriptions |
| `max_length` | integer | `2000` | Maximum characters of changelog to embed |

Set `GITHUB_TOKEN` env var to avoid GitHub API rate limits.

### `[vulnerability]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Query [OSV](https://osv.dev) for CVEs on updated dependencies |
| `security_labels` | string[] | `["security"]` | Labels added to MRs that fix known CVEs |
| `priority_boost` | bool | `true` | Security updates bypass rate limits and schedule windows |

### `[lockfile]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Update `Chart.lock` alongside `Chart.yaml` |
| `helm_binary` | string | — | Path to `helm` binary; omit to use built-in SHA256 digest computation |

### `[replacement]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Detect deprecated/renamed images and charts |
| `rules_file` | string | — | Path to a TOML file with custom replacement rules |
| `warn_only` | bool | `false` | Log a warning instead of opening a replacement MR |

### `[registries."<host>"]`

Configures credentials for a private container registry or Helm repository.

| Key | Type | Description |
|-----|------|-------------|
| `username` | string | Registry username (optional for token-only auth) |
| `password_env` | string | Name of the env var containing the password or API key |
| `base_url` | string | Override the registry base URL (useful for proxies or testing) |

**Artifactory OCI pattern** — when `password_env` is set but `username` is omitted, the credential is sent as a Bearer token (API key pre-auth).

### `[[regex_managers]]`

Defines a custom manager using a regex to extract dependency names and versions from arbitrary files. See [Managers](managers.md#regex-manager) for full details.

| Key | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | string | yes | Identifier used in logs and branch names |
| `file_patterns` | string[] | yes | Glob patterns for files to scan |
| `match_pattern` | string | yes | Regex with named groups `depName` and `currentValue` |
| `datasource` | string | yes | `docker`, `helm-oci`, or `helm-repo` |
| `registry_url` | string | no | Registry/repo URL when not captured by the regex |
| `versioning` | string | no | Versioning scheme hint (informational) |
