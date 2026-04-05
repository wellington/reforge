# Renovate-RS: Rust Implementation Plan

A focused Rust reimplementation of Renovate's core functionality, scoped to **Helm charts** and **Dockerfiles/Docker Compose**, targeting **self-managed GitLab** as the platform backend.

---

## Project Goals

1. Parse Helm charts and Docker files to extract dependency declarations with pinned versions.
2. Query upstream registries (OCI/Docker registries, Helm chart repositories) for newer versions.
3. Open GitLab Merge Requests with version bump diffs, one MR per dependency (or grouped by policy).
4. Run as a standalone CLI binary suitable for GitLab CI scheduled pipelines.
5. Serve as a Rust learning vehicle — favor clarity and idiomatic Rust over premature abstraction.

---

## Architecture Overview

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  CLI Entry   │────▶│  Platform Client  │────▶│  GitLab API v4  │
│  (clap)      │     │  (reqwest/async)  │     │                 │
└─────┬───────┘     └──────────────────┘     └─────────────────┘
      │
      ▼
┌─────────────────┐
│  Repo Scanner    │  Walks repo tree, finds managed files
└─────┬───────────┘
      │
      ▼
┌─────────────────────────────────────────┐
│  Manager Trait Implementations           │
│  ┌───────────────┐  ┌─────────────────┐ │
│  │ HelmManager   │  │ DockerManager   │ │
│  │ - Chart.yaml  │  │ - Dockerfile    │ │
│  │ - values.yaml │  │ - compose.yaml  │ │
│  └───────┬───────┘  └───────┬─────────┘ │
└──────────┼──────────────────┼───────────┘
           ▼                  ▼
┌─────────────────────────────────────────┐
│  Registry Clients                        │
│  ┌────────────────┐ ┌────────────────┐  │
│  │ OCI / Docker   │ │ Helm Repo      │  │
│  │ Registry v2    │ │ (index.yaml)   │  │
│  └────────────────┘ └────────────────┘  │
└─────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────┐
│  Update Engine                           │
│  - Version comparison (semver)           │
│  - Diff generation                       │
│  - MR creation via Platform Client       │
└─────────────────────────────────────────┘
```

---

## Crate Dependencies

| Crate | Purpose |
|---|---|
| `clap` | CLI argument parsing with derive macros |
| `tokio` | Async runtime |
| `reqwest` | HTTP client (GitLab API, registry API) |
| `serde` / `serde_json` / `serde_yaml` | Serialization for YAML (Helm) and JSON (API, registry) |
| `semver` | Semver parsing and comparison |
| `regex` | Dockerfile `FROM` line parsing, image tag extraction |
| `tracing` / `tracing-subscriber` | Structured logging |
| `thiserror` / `anyhow` | Error handling |
| `toml` | Config file parsing (project config) |
| `tempfile` | Scratch space for file manipulation during updates |
| `base64` | Docker registry auth token handling |

---

## Module Breakdown

### 1. `main.rs` — CLI Entry Point

Use `clap` derive to define the CLI interface.

```
renovate-rs [OPTIONS]
    --config <path>          Path to config file (default: renovate-rs.toml)
    --repo <group/project>   GitLab project path (overrides config)
    --dry-run                Log what would be done without creating MRs
    --log-level <level>      trace | debug | info | warn | error
    --token <token>          GitLab API token (prefer env: RENOVATE_RS_TOKEN)
    --gitlab-url <url>       GitLab instance URL (prefer env: RENOVATE_RS_GITLAB_URL)
```

Responsibilities:
- Parse CLI args, layer with env vars, layer with config file (in that precedence).
- Initialize tracing subscriber.
- Build shared config struct, hand off to orchestrator.

### 2. `config.rs` — Configuration

Define a `Config` struct deserialized from TOML:

```toml
[gitlab]
url = "https://gitlab.example.com"
# token loaded from RENOVATE_RS_TOKEN env var

[scan]
# List of GitLab project paths to scan
projects = ["procore-fed/traffic/api-gateway"]

[managers]
enabled = ["helm", "docker"]

[versioning]
# Pin strategy: "semver-minor" only bumps minor+patch, "semver-major" bumps everything
pin_strategy = "semver-minor"

[merge_request]
# Branch prefix for update branches
branch_prefix = "renovate-rs/"
# Labels to apply to MRs
labels = ["renovate", "automated"]
# Group updates into a single MR per manager, or one MR per dependency
grouping = "per-dependency"
# Assignee GitLab user IDs (optional)
assignees = []
# Auto-merge if pipeline passes (sets merge_when_pipeline_succeeds)
auto_merge = false

[registries]
# Additional registry auth for private registries
# Each entry maps a registry host to credentials

[registries."registry.example.com"]
username = "deploy-token"
# password loaded from env: RENOVATE_RS_REGISTRY_EXAMPLE_PASSWORD
password_env = "RENOVATE_RS_REGISTRY_EXAMPLE_PASSWORD"

[registries."artifactory.example.com"]
username = "deploy-token"
password_env = "RENOVATE_RS_ARTIFACTORY_PASSWORD"
```

### 3. `platform/gitlab.rs` — GitLab API Client

A struct `GitLabClient` wrapping `reqwest::Client` with the base URL and auth token.

Methods needed:

```rust
impl GitLabClient {
    pub async fn new(url: &str, token: &str) -> Result<Self>;

    // Repository file access
    pub async fn get_file(&self, project: &str, path: &str, ref_: &str) -> Result<String>;
    pub async fn list_tree(&self, project: &str, ref_: &str, path: Option<&str>, recursive: bool) -> Result<Vec<TreeEntry>>;

    // Branch management
    pub async fn create_branch(&self, project: &str, branch: &str, ref_: &str) -> Result<()>;
    pub async fn delete_branch(&self, project: &str, branch: &str) -> Result<()>;
    pub async fn branch_exists(&self, project: &str, branch: &str) -> Result<bool>;

    // File commits
    pub async fn commit_files(&self, project: &str, branch: &str, message: &str, actions: Vec<CommitAction>) -> Result<()>;

    // Merge request management
    pub async fn create_mr(&self, project: &str, params: CreateMrParams) -> Result<MergeRequest>;
    pub async fn list_open_mrs(&self, project: &str, source_branch_prefix: Option<&str>) -> Result<Vec<MergeRequest>>;
    pub async fn update_mr(&self, project: &str, mr_iid: u64, params: UpdateMrParams) -> Result<()>;
    pub async fn close_mr(&self, project: &str, mr_iid: u64) -> Result<()>;

    // Default branch detection
    pub async fn get_default_branch(&self, project: &str) -> Result<String>;
}
```

Use GitLab API v4 endpoints. URL-encode project paths with `%2F`. Rate limit awareness: respect `Retry-After` headers. Implement a simple retry loop (3 attempts with exponential backoff) for transient 5xx errors.

### 4. `manager/mod.rs` — Manager Trait

```rust
/// A detected dependency in a managed file.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Human-readable name, e.g. "nginx" or "ingress-nginx"
    pub name: String,
    /// The current version string as found in the file
    pub current_version: String,
    /// The registry/source to query for updates
    pub registry: RegistrySource,
    /// File path where this dependency was found
    pub file_path: String,
    /// Enough context to perform an in-place update in the file
    pub update_context: UpdateContext,
}

#[derive(Debug, Clone)]
pub enum RegistrySource {
    DockerRegistry { image: String, registry: Option<String> },
    HelmRepository { repo_url: String, chart_name: String },
    OciHelmRegistry { image: String, registry: Option<String> },
}

#[derive(Debug, Clone)]
pub enum UpdateContext {
    /// For YAML files: the key path to the version value
    YamlKeyPath { keys: Vec<String> },
    /// For Dockerfiles: line number and image reference pattern
    DockerFrom { line_number: usize, full_reference: String },
}

#[async_trait]
pub trait PackageManager: Send + Sync {
    /// Manager name for logging and config
    fn name(&self) -> &'static str;

    /// Filename patterns this manager is interested in
    fn file_patterns(&self) -> Vec<&'static str>;

    /// Given file contents, extract all managed dependencies
    fn extract_dependencies(&self, file_path: &str, contents: &str) -> Result<Vec<Dependency>>;
}
```

### 5. `manager/docker.rs` — Docker Manager

Handles:
- `Dockerfile` (and `Dockerfile.*` variants)
- `docker-compose.yml` / `docker-compose.yaml` / `compose.yml` / `compose.yaml`

**Dockerfile parsing:**

Parse `FROM` instructions. Handle:
- `FROM nginx:1.25.3`
- `FROM nginx:1.25.3 AS builder`
- `FROM registry.example.com/myorg/myimage:v2.1.0`
- `FROM --platform=linux/amd64 nginx:1.25.3`
- Multi-stage builds (extract from every `FROM` line)
- `ARG`-based image references: `ARG BASE_IMAGE=nginx:1.25.3` followed by `FROM ${BASE_IMAGE}` — extract the version from the `ARG` line, note this as the update target.

Use regex for initial extraction. Pattern for FROM lines:
```
^FROM\s+(?:--platform=\S+\s+)?(?P<image>[^\s]+?)(?::(?P<tag>[^\s@]+))?(?:@(?P<digest>sha256:\w+))?\s*(?:AS\s+\S+)?$
```

For images pinned by digest only (no tag), skip — we cannot determine version intent.

**Docker Compose parsing:**

Parse YAML, walk all `services.*.image` values. Extract image and tag from the string.
Also handle `services.*.build.args` for ARG-based image pinning if present.

### 6. `manager/helm.rs` — Helm Manager

Handles:
- `Chart.yaml` — `dependencies[].version` and `dependencies[].repository`
- `values.yaml` / `values-*.yaml` — `image.tag`, `image.repository` patterns (convention-based)

**Chart.yaml parsing:**

Deserialize into a struct:
```rust
#[derive(Deserialize)]
struct ChartYaml {
    dependencies: Option<Vec<ChartDependency>>,
}

#[derive(Deserialize)]
struct ChartDependency {
    name: String,
    version: String,
    repository: String,
    // condition, tags, etc. — not needed for version bumping
}
```

For each dependency:
- If `repository` starts with `https://` or `http://`, it is a classic Helm repo — fetch `index.yaml`.
- If `repository` starts with `oci://`, it is an OCI-based Helm chart — use Docker Registry v2 API to list tags.
- If `repository` starts with `alias:` or `@`, resolve from the repo's `Chart.yaml` or skip with a warning.

**values.yaml image tag extraction:**

This is convention-based and inherently fuzzy. Look for common patterns:
```yaml
image:
  repository: nginx
  tag: "1.25.3"
```

Walk the YAML tree looking for any mapping that contains both a `repository` (or `image`) key and a `tag` (or `version`) key as siblings. Extract as a Docker image dependency.

Use `serde_yaml::Value` for dynamic traversal rather than rigid struct deserialization, since values files are arbitrarily structured.

### 7. `registry/mod.rs` — Registry Trait and Implementations

```rust
#[async_trait]
pub trait RegistryClient: Send + Sync {
    /// Fetch all available versions for a given dependency
    async fn fetch_versions(&self, source: &RegistrySource) -> Result<Vec<Version>>;
}
```

Where `Version` wraps `semver::Version` but also retains the original string (registries don't always use strict semver).

#### 7a. `registry/docker.rs` — Docker Registry v2

Implements the OCI Distribution Spec for listing tags.

Flow:
1. `GET /v2/` — check connectivity and auth requirements.
2. If 401 with `Www-Authenticate` header, perform token auth:
   - Parse realm, service, scope from the header.
   - `GET <realm>?service=<service>&scope=<scope>` with Basic auth if credentials are configured.
   - Use returned Bearer token for subsequent requests.
3. `GET /v2/<name>/tags/list` — returns `{ "tags": ["1.25.0", "1.25.1", ...] }`.
4. Filter tags that parse as semver (skip `latest`, `alpine`, `slim`, etc. unless they embed a version).

Handle Docker Hub specially: image names without a registry prefix resolve to `registry-1.docker.io`, and library images like `nginx` resolve to `library/nginx`.

Pagination: follow `Link` header if present for large tag lists.

#### 7b. `registry/helm.rs` — Helm Repository

For classic Helm repos:
1. `GET <repo_url>/index.yaml` — this is a large YAML file listing all charts and their versions.
2. Deserialize, find the entry matching `chart_name`.
3. Extract all versions from the entry's version list.

For OCI-based Helm registries:
- Reuse the Docker registry client — Helm OCI charts are stored as OCI artifacts. The chart name maps to a repository, and versions map to tags.

### 8. `versioning.rs` — Version Comparison

```rust
pub struct VersionPolicy {
    pub strategy: PinStrategy,
}

pub enum PinStrategy {
    /// Update to latest within same major
    SemverMinor,
    /// Update to latest including major bumps
    SemverMajor,
    /// Update to latest patch only
    SemverPatch,
}

impl VersionPolicy {
    /// Given a current version and a list of available versions,
    /// return the best update candidate (or None if already up to date).
    pub fn best_update(
        &self,
        current: &semver::Version,
        available: &[semver::Version],
    ) -> Option<semver::Version>;
}
```

Sort available versions descending, filter by policy constraints, return the highest match.

Handle version strings that have a `v` prefix (`v1.2.3`) — strip for comparison, preserve in output.

### 9. `updater.rs` — File Update Engine

Given a `Dependency` and a new version string, produce the updated file content.

For YAML files (Helm Chart.yaml, values.yaml, Compose):
- Use string-based find-and-replace rather than deserialize-modify-serialize, to preserve comments, formatting, and key ordering. Load the file as a string, locate the exact line/value using the `UpdateContext`, and perform a targeted replacement.

For Dockerfiles:
- Replace on the specific line identified during extraction.

Return a `FileUpdate` struct:
```rust
pub struct FileUpdate {
    pub file_path: String,
    pub original_content: String,
    pub updated_content: String,
}
```

### 10. `orchestrator.rs` — Main Workflow

This is the top-level async function that ties everything together.

```
for each project in config.projects:
    1. Detect default branch
    2. List repo tree from default branch
    3. Filter tree for files matching enabled managers' file_patterns()
    4. For each matching file:
        a. Fetch file contents via GitLab API
        b. Call manager.extract_dependencies()
    5. Deduplicate dependencies (same image across multiple files)
    6. For each unique dependency:
        a. Call registry_client.fetch_versions()
        b. Call version_policy.best_update()
    7. Filter to dependencies that have an available update
    8. Check existing open MRs (by branch name convention) to avoid duplicates
    9. For each update (or group, per config):
        a. Generate file updates via updater
        b. Create branch: renovate-rs/<manager>-<dep_name>-<new_version>
        c. Commit updated files to the branch
        d. Create MR with descriptive title and body
        e. If auto_merge enabled, set merge_when_pipeline_succeeds
    10. Optionally: close stale MRs for versions that are no longer the target
```

**MR body template:**

```markdown
## Dependency Update

| Package | Manager | Current | New |
|---------|---------|---------|-----|
| {name}  | {manager} | {current} | {new} |

---

*This MR was automatically created by renovate-rs.*
```

### 11. `error.rs` — Error Types

Use `thiserror` for library-style errors:

```rust
#[derive(thiserror::Error, Debug)]
pub enum RenovateError {
    #[error("GitLab API error: {status} {message}")]
    GitLabApi { status: u16, message: String },

    #[error("Registry error for {registry}: {message}")]
    Registry { registry: String, message: String },

    #[error("Failed to parse {file}: {reason}")]
    Parse { file: String, reason: String },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

---

## Project Structure

```
renovate-rs/
├── Cargo.toml
├── renovate-rs.toml.example
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── orchestrator.rs
│   ├── updater.rs
│   ├── versioning.rs
│   ├── error.rs
│   ├── platform/
│   │   ├── mod.rs          # Platform trait (future: GitHub, Bitbucket)
│   │   └── gitlab.rs
│   ├── manager/
│   │   ├── mod.rs          # PackageManager trait + Dependency types
│   │   ├── docker.rs
│   │   └── helm.rs
│   └── registry/
│       ├── mod.rs          # RegistryClient trait
│       ├── docker.rs       # Docker/OCI registry v2
│       └── helm.rs         # Helm classic repo (index.yaml)
└── tests/
    ├── fixtures/
    │   ├── Dockerfile.simple
    │   ├── Dockerfile.multistage
    │   ├── Dockerfile.arg_based
    │   ├── docker-compose.yaml
    │   ├── Chart.yaml
    │   ├── values.yaml
    │   └── helm-index.yaml     # Sample Helm repo index for testing
    ├── docker_manager_test.rs
    ├── helm_manager_test.rs
    ├── versioning_test.rs
    └── integration/
        └── gitlab_mock_test.rs  # Integration test with mock HTTP server
```

---

## Implementation Phases

### Phase 1: Foundation (skeleton + Docker manager + dry-run)

**Deliverables:** CLI that scans a GitLab repo, extracts Docker image references, queries Docker Hub for newer tags, and prints a dry-run report.

1. Scaffold project with `cargo init`, add dependencies to `Cargo.toml`.
2. Implement `config.rs` — config file parsing + env var overlay.
3. Implement `platform/gitlab.rs` — `get_file`, `list_tree`, `get_default_branch`.
4. Implement `manager/docker.rs` — Dockerfile and Compose parsing.
5. Implement `registry/docker.rs` — Docker Hub tag listing (public images only, no auth yet).
6. Implement `versioning.rs` — semver comparison with `SemverMinor` policy.
7. Implement `orchestrator.rs` — scan + extract + lookup loop, dry-run output to stdout.
8. Write unit tests for Dockerfile parsing (fixtures) and version comparison.

### Phase 2: Helm Manager + MR Creation

**Deliverables:** Helm Chart.yaml and values.yaml support. Actual MR creation on GitLab.

1. Implement `manager/helm.rs` — Chart.yaml dependency parsing.
2. Implement `registry/helm.rs` — Helm index.yaml fetching and parsing.
3. Implement `manager/helm.rs` — values.yaml image tag extraction (convention-based).
4. Implement OCI Helm registry support (reuse Docker registry client).
5. Implement `updater.rs` — string-based file updates for YAML and Dockerfiles.
6. Extend `platform/gitlab.rs` — `create_branch`, `commit_files`, `create_mr`, `list_open_mrs`.
7. Wire MR creation into orchestrator.
8. Implement duplicate MR detection (check for existing branch before creating).
9. Write integration test using a mock HTTP server (`wiremock` crate) for the full flow.

### Phase 3: Private Registry Auth + Polish

**Deliverables:** Private registry support, stale MR cleanup, robustness.

1. Implement Docker v2 token auth flow (Bearer + Basic).
2. Implement registry credential loading from config + env vars.
3. Add Artifactory / private Helm repo auth support.
4. Implement stale MR closing (when a newer version supersedes a pending MR).
5. Add `--json` output mode for dry-run (machine-readable).
6. Add structured tracing spans per-project and per-dependency for observability.
7. Handle edge cases: digests-only pinning, non-semver tags, unreachable registries (graceful skip with warning).
8. Write a sample `.gitlab-ci.yml` for running renovate-rs as a scheduled pipeline:

```yaml
renovate-rs:
  stage: maintenance
  image: rust:1.78  # or publish a pre-built container image
  variables:
    RENOVATE_RS_TOKEN: $GITLAB_RENOVATE_TOKEN
    RENOVATE_RS_GITLAB_URL: $CI_SERVER_URL
  script:
    - ./renovate-rs --config renovate-rs.toml
  rules:
    - if: $CI_PIPELINE_SOURCE == "schedule"
```

### Phase 4: Stretch Goals (optional)

- **Automerge logic**: Set `merge_when_pipeline_succeeds` on MRs matching policy.
- **Dependency dashboard**: Create/update a single GitLab issue summarizing all pending updates across repos.
- **Rate limiting**: Token bucket for registry API calls to avoid throttling.
- **Caching**: Cache registry lookups to a local file between runs to reduce API calls.
- **Additional managers**: Terraform provider versions, GitHub Actions (for cross-platform use later).
- **Platform abstraction**: Trait-based platform client so GitHub support can be added alongside GitLab.

---

## Testing Strategy

- **Unit tests**: Every parser (Dockerfile, Compose, Chart.yaml, values.yaml) gets fixture-based tests. Version comparison logic gets property-based tests if time permits (`proptest` crate).
- **Integration tests**: Use `wiremock` to stand up mock HTTP servers simulating GitLab API and Docker registry responses. Run the full orchestrator against mocks.
- **Manual validation**: Before Phase 2 MR creation goes live, run with `--dry-run` against a real repo and verify output. Then test MR creation against a throwaway GitLab project.

---

## Key Design Decisions

1. **String-based file updates, not AST round-tripping.** YAML round-trip libraries lose comments and reorder keys. Doing targeted string replacement preserves the original file formatting, which matters for review-friendly diffs.

2. **One MR per dependency by default.** This matches Renovate's default behavior and makes it easy to approve/reject individual updates. Grouping is configurable but not the default.

3. **Async from the start.** Registry and GitLab API calls are the bottleneck. Using `tokio` + `reqwest` allows concurrent lookups across dependencies within a project. Use `futures::stream::buffered()` to cap concurrency (e.g., 5 concurrent registry lookups).

4. **No local git clone.** Operate entirely through the GitLab API (file reads via Repository Files API, commits via Commits API). This avoids needing git installed, keeps the binary self-contained, and works naturally in ephemeral CI environments.

5. **Config file is optional.** Everything can be driven by CLI flags and env vars for simple single-repo use cases. The config file adds multi-repo support and registry credentials.
