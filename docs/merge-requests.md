# Merge Requests

## Default Behavior

By default, reforge opens **one merge request per dependency** when a newer version is detected. Each MR:

- Creates a branch named `reforge/<dep-name>-<version>` (prefix is configurable)
- Applies labels `reforge` and `automated`
- Embeds a changelog section (if enabled and available)
- Includes CVE details (if vulnerability scanning is enabled)
- Is idempotent — if an MR for the same update already exists, reforge skips it

## Branch Naming

Branches follow the pattern:

```
<branch_prefix><dep-name>-<new-version>
```

Default: `reforge/nginx-1.29.0`

Change the prefix:

```toml
[merge_request]
branch_prefix = "deps/"
```

## Labels and Assignees

```toml
[merge_request]
labels    = ["reforge", "automated", "dependencies"]
assignees = [42, 87]   # GitLab user IDs
```

## Stale MR Handling

When a previously-opened reforge MR falls behind the default branch (due to other commits merging), reforge can automatically remediate it. Configure via `stale_mr_strategy`:

| Strategy | Behavior |
|----------|----------|
| `rebase` | Rebase the branch onto the current default branch |
| `recreate` | Close the old MR, delete the branch, and open a fresh MR |
| `ignore` | Leave stale MRs untouched |

```toml
[merge_request]
rebase_enabled    = true
stale_mr_strategy = "rebase"   # default
```

Disable staleness checks entirely:

```toml
[merge_request]
rebase_enabled = false
```

## Rate Limiting

Limit the number of open reforge MRs at any time to avoid overwhelming your reviewers:

```toml
[merge_request]
max_open_mrs = 10
```

When the cap is reached, new updates are detected but no MR is opened. The Dependency Dashboard issue still lists them as pending.

## Grouping

### Default: per-dependency

Each dependency gets its own MR. This is the default and matches Renovate's behavior.

### Named Grouping Rules

Use `grouping_rules` to combine related updates into a single MR. Rules are evaluated in order; the first matching rule wins for each candidate.

```toml
[[merge_request.grouping_rules]]
name           = "infra-charts"
match_patterns = ["prometheus", "grafana", "loki"]
group_by       = "pattern"     # all three go into one MR

[[merge_request.grouping_rules]]
name           = "docker-patch-updates"
match_patterns = []            # empty = match all
group_by       = "update_type" # separate groups per patch/minor/major
separate_major = true          # majors always get their own MR
```

#### `group_by` values

| Value | Behavior |
|-------|----------|
| `pattern` | All matched candidates go into one MR named after the rule |
| `update_type` | Creates separate MRs per semver bump type (patch / minor / major) |
| `manager` | Creates separate MRs per package manager (docker / helm) |
| `path` | Creates separate MRs per directory containing the dependency file |

#### `separate_major`

When `true`, major version bumps are always split into their own MR, regardless of `group_by`. Useful to get automatic patch/minor updates while requiring explicit review for major bumps.

---

## Automerge

### Global automerge

Enable automerge for all MRs (sets GitLab's "merge when pipeline succeeds"):

```toml
[merge_request]
auto_merge = true
```

### Per-dependency policies

Fine-grained control with `automerge_policies`. Policies are evaluated in order; the first match wins.

```toml
# Automerge all patch updates for nginx automatically
[[merge_request.automerge_policies]]
match_pattern = "nginx"
update_types  = ["patch"]
enabled       = true

# Automerge any update to the curl image, but only after 2 days
[[merge_request.automerge_policies]]
match_pattern    = "curlimages/curl"
update_types     = []          # empty = all types
enabled          = true
minimum_age_days = 2

# Never automerge major updates to vault
[[merge_request.automerge_policies]]
match_pattern = "hashicorp/vault"
update_types  = ["major"]
enabled       = false

# Automerge everything else at patch level
[[merge_request.automerge_policies]]
match_pattern = "*"
update_types  = ["patch"]
enabled       = true
```

#### Policy fields

| Field | Type | Description |
|-------|------|-------------|
| `match_pattern` | string | Glob matched against the dependency name (`*` matches all) |
| `update_types` | string[] | `patch`, `minor`, `major`; empty means all types |
| `enabled` | bool | Whether automerge is active for matches |
| `minimum_age_days` | integer | Minimum number of days before the MR may be automerged |

### `minimum_age_days`

This acts as a stabilization window. Reforge checks the MR's creation timestamp on each run; the MR is only automerged once it has been open for at least the specified number of days. This gives time for downstream breakage to surface before a change is automatically landed.

---

## Dependency Dashboard

Reforge maintains a GitLab issue titled **"Dependency Dashboard"** that provides an overview of all dependency update activity.

The dashboard includes:

- All open reforge MRs and their status
- Pending updates that are queued but not yet opened (e.g. because `max_open_mrs` was reached)
- The last run timestamp

Configure the dashboard:

```toml
[dashboard]
enabled = true
labels  = ["reforge", "dependency-dashboard"]
```

Disable it entirely:

```bash
reforge --no-dashboard
```

Or in config:

```toml
[dashboard]
enabled = false
```
