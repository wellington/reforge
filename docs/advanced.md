# Advanced Features

## Schedule Windows

Restrict when new MRs may be created to avoid noise outside business hours.

```toml
[merge_request.schedule_window]
days        = ["monday", "tuesday", "wednesday", "thursday", "friday"]
hours_start = 9    # UTC hour (inclusive)
hours_end   = 17   # UTC hour (exclusive)
```

Valid day values: `monday`, `tuesday`, `wednesday`, `thursday`, `friday`, `saturday`, `sunday`.

When reforge runs outside the configured window, it detects updates and records them in the Dependency Dashboard but does not open new MRs. The next run within the window will open them.

**Security updates bypass the schedule** by default (controlled by `vulnerability.priority_boost`). A CVE fix is never held back.

---

## Vulnerability Awareness

Reforge can query the [OSV (Open Source Vulnerabilities)](https://osv.dev) database to check whether a detected update fixes a known CVE.

```toml
[vulnerability]
enabled         = true
security_labels = ["security", "cve"]
priority_boost  = true
```

When a vulnerability is found:

- The MR description includes CVE IDs, severity, and a summary of the vulnerability.
- The labels in `security_labels` are applied to the MR in addition to the standard labels.
- When `priority_boost = true`, the update bypasses `max_open_mrs` rate limits and schedule windows.

This feature works for Docker images whose names correspond to OSV package entries. Coverage is best for well-known images and Helm charts with upstream GitHub releases.

---

## Changelogs in Merge Requests

Reforge fetches GitHub Release notes and embeds them as a collapsible section in the MR description.

```toml
[changelog]
enabled    = true
max_length = 2000   # characters; longer changelogs are truncated
```

Set `GITHUB_TOKEN` to avoid hitting GitHub's unauthenticated rate limit (60 req/hour):

```bash
export GITHUB_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

The changelog section is rendered as a `<details>` block so it does not clutter the MR description for large release notes.

---

## Replacement and Deprecation Detection

Reforge detects when a dependency has been renamed, moved to a different registry, or deprecated — and opens a migration MR.

### Built-in Rules

Reforge ships with built-in rules for common image migrations:

- `gcr.io/google-containers/*` → `registry.k8s.io/*`
- `k8s.gcr.io/*` → `registry.k8s.io/*`
- `docker.io/library/nginx` → `nginxinc/nginx-unprivileged` (rootless alternative)
- Bitnami Helm charts to OCI format

### Custom Rules

Add your own rules in a TOML file:

```toml
# my-replacements.toml

[[rules]]
old_name     = "internal-proxy"
new_name     = "secure-proxy"
old_registry = "registry.legacy.example.com"
new_registry = "registry.example.com"
reason       = "internal-proxy has been renamed to secure-proxy."

[[rules]]
old_name        = "abandoned-image"
new_name        = "abandoned-image"
old_registry    = "docker.io"
deprecated_only = true
reason          = "This image is no longer maintained; no direct replacement exists."
```

Reference the file from `reforge.toml`:

```toml
[replacement]
rules_file = "my-replacements.toml"
```

### Rule Fields

| Field | Type | Description |
|-------|------|-------------|
| `old_name` | string | Image/chart name to match; use `*` as a suffix wildcard |
| `new_name` | string | Replacement name; use `*` to carry the wildcard suffix forward |
| `old_registry` | string | Optional registry prefix to constrain matching |
| `new_registry` | string | Registry for the replacement image/chart |
| `reason` | string | Human-readable note included in the MR description |
| `deprecated_only` | bool | When `true`, only emit a warning — no replacement MR is created |

### Warn-Only Mode

If you want to detect replacements but not automatically create MRs:

```toml
[replacement]
warn_only = true
```

Reforge logs a warning and notes the deprecation in the Dependency Dashboard.

---

## Lock File Maintenance

When reforge updates a Helm chart version in `Chart.yaml`, it also updates the corresponding entry in `Chart.lock`.

```toml
[lockfile]
enabled = true
```

### Digest Computation

By default, reforge computes SHA256 digests of downloaded chart tarballs without invoking the `helm` CLI. This works in environments where `helm` is not installed.

To use the `helm` binary instead:

```toml
[lockfile]
helm_binary = "/usr/local/bin/helm"
```

### Disabling Lock File Updates

If your workflow regenerates `Chart.lock` in CI independently:

```toml
[lockfile]
enabled = false
```

---

## TLS and Self-Signed Certificates

For GitLab instances with self-signed TLS certificates:

```toml
[gitlab]
insecure = true
```

> Use this only in internal environments. It disables all TLS certificate verification for GitLab API calls.

---

## JSON Output

In dry-run mode, output results as machine-readable JSON:

```bash
reforge --dry-run --json
```

This is useful for integrating reforge into scripts, dashboards, or custom tooling that consumes the update candidate list.

---

## Log Levels

Control verbosity with `--log-level`:

```bash
reforge --log-level debug   # very verbose, useful for diagnosing registry auth issues
reforge --log-level warn    # only warnings and errors
reforge --log-level trace   # maximum verbosity including HTTP request details
```

The default is `info`. Log format is structured (timestamp + level + message) and goes to stderr.
