# Reforge: Deployment Implementation Plan

Deploy reforge as a container image and configure the `poc/configurations` GitLab repo as the first managed target. This covers building, publishing, repo population, and CI integration.

---

## Context

- **Reforge project**: `/home/drew/src/github.com/procore/reforge/` (Rust binary, compiles to `reforge`)
- **Configurations repo**: `https://gitlab.mgmt.procoregov-qa.internal/poc/configurations.git` (project ID 4)
  - Local checkout: `/home/drew/src/gitlab.mgmt.procoregov-qa.internal/poc/configurations`
  - Currently empty (just README.md on main)
  - Container registry enabled at `gitlab.mgmt.procoregov-qa.internal:5050/poc/configurations`
  - Shared runners enabled
  - Git credentials configured for HTTPS push (username `oauth2`, PAT stored in git credential helper)
- **Reforge bot**: GitLab user `project_4_bot_0f5c1124e1855033e64fa7a867b776ed` (name: "reforge", user ID 6)
- **OCI chart registry**: `oci://oci-charts.artifacts.procoretech.com` is accessible via Helm with auth tokens
- **Reference branches** (in local `govcloud-qa-poc` checkout):
  - `feature/GOVENG-375-app-cluster` — ApplicationSet + multi-cluster pattern
  - `feature/login-upstream-chart` — Login app chart and values

---

## Phase 1: Build and Publish Reforge Container Image

### 1a. Create Dockerfile

Add a multi-stage Dockerfile to the reforge project.

```dockerfile
FROM rust:1.86-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/reforge /usr/local/bin/reforge
ENTRYPOINT ["reforge"]
```

Notes:
- `ca-certificates` is required for TLS to GitLab and registry APIs.
- No git binary needed — reforge operates entirely through the GitLab API.
- The `Cargo.lock` must exist (run `cargo generate-lockfile` if missing).

### 1b. Build the image locally

```bash
cd /home/drew/src/github.com/procore/reforge
cargo generate-lockfile  # ensure Cargo.lock exists
docker build -t gitlab.mgmt.procoregov-qa.internal:5050/poc/configurations/reforge:latest .
docker tag gitlab.mgmt.procoregov-qa.internal:5050/poc/configurations/reforge:latest \
           gitlab.mgmt.procoregov-qa.internal:5050/poc/configurations/reforge:0.1.0
```

### 1c. Push to GitLab container registry

```bash
# Authenticate using the git credential PAT
docker login gitlab.mgmt.procoregov-qa.internal:5050 -u oauth2 -p <PAT>

docker push gitlab.mgmt.procoregov-qa.internal:5050/poc/configurations/reforge:latest
docker push gitlab.mgmt.procoregov-qa.internal:5050/poc/configurations/reforge:0.1.0
```

---

## Phase 2: Populate the Configurations Repo

### Target structure

```
poc/configurations/
├── appsets/
│   └── cluster-apps.yaml            # ApplicationSet definition
├── apps/
│   ├── app/
│   │   └── login.yaml               # Login — app clusters only
│   └── tooling/
│       └── .gitkeep                  # No tooling-only apps yet
├── values/
│   └── login/
│       ├── values.yaml              # Base login values (govcloud-qa config)
│       └── values-app.yaml          # App-cluster-specific overrides
├── reforge.toml                     # Reforge configuration
├── .gitlab-ci.yml                   # CI pipeline definition
└── README.md                        # Updated project README
```

### 2a. ApplicationSet: `appsets/cluster-apps.yaml`

Modeled on `govcloud-qa-poc:feature/GOVENG-375-app-cluster:argocd-apps/appsets/cluster-apps.yaml`.

Key differences from the reference:
- `repoURL` → `https://gitlab.mgmt.procoregov-qa.internal/poc/configurations.git`
- `revision` → `main`
- File paths → `apps/tooling/*.yaml` and `apps/app/*.yaml` (no `argocd-apps/` prefix since it's the repo root)

The ApplicationSet uses matrix generators:
- **Tooling matrix**: git files from `apps/tooling/*.yaml` × clusters with label `cluster-role: tooling`
- **App matrix**: git files from `apps/app/*.yaml` × clusters with label `cluster-role: app`

Template uses the CMP plugin `helm-with-cluster-values` which:
1. Reads `helmChart` / `helmVersion` from the app YAML to pull upstream OCI charts
2. Looks for values at `values/<appName>/values.yaml` and `values/<appName>/values-<clusterEnv>.yaml`
3. Substitutes cluster environment placeholders (`__clusterName__`, etc.)

### 2b. Login app definition: `apps/app/login.yaml`

```yaml
appName: login
chartPath: charts/login
namespace: login
helmChart: "oci://oci-charts.artifacts.procoretech.com/developer-excellence/app-charts/stateless-http-service"
helmVersion: "14.1.0"
```

This file **only** exists under `apps/app/` (not `apps/tooling/`), so the ApplicationSet will only generate an Application for clusters with `cluster-role: app`.

The `helmChart` and `helmVersion` fields tell the CMP plugin to pull the upstream `stateless-http-service` chart at version `14.1.0` from the Procore OCI registry. The OCI registry auth is already configured in ArgoCD's CMP sidecar.

### 2c. Login values files

**`values/login/values.yaml`** — Base configuration extracted from `govcloud-qa-poc:feature/login-upstream-chart:charts/login/values-govcloud-qa.yaml`. Contains:
- Subchart toggles (disable stateless-http-service, sidekiq, tugboat-helpers, external-secrets, etc. since backing infra doesn't exist in this environment)
- Standalone deployment config (web/worker replicas, resources, env vars)
- Local database and Redis config
- Virtual service config for `login.mgmt.procoregov-qa.internal`

**`values/login/values-app.yaml`** — App-cluster-specific overrides. Initially minimal (can be empty or contain cluster-specific resource adjustments).

### 2d. Commit and push

All files committed to `main` branch of `poc/configurations` via the local checkout at `/home/drew/src/gitlab.mgmt.procoregov-qa.internal/poc/configurations`.

---

## Phase 3: Configure Reforge CI Pipeline

### 3a. Reforge config: `reforge.toml`

```toml
[gitlab]
url = "https://gitlab.mgmt.procoregov-qa.internal"

[scan]
projects = ["poc/configurations"]

[managers]
enabled = ["helm"]

[versioning]
pin_strategy = "semver-minor"

[merge_request]
branch_prefix = "reforge/"
labels = ["reforge", "automated"]
grouping = "per-dependency"
auto_merge = false
```

Reforge will scan this repo's YAML files for Helm chart dependencies (the `helmVersion` fields in app definitions and any `Chart.yaml` dependencies) and OCI image tags, then open MRs when newer versions are available.

### 3b. GitLab CI: `.gitlab-ci.yml`

```yaml
stages:
  - maintenance

reforge:
  stage: maintenance
  image: gitlab.mgmt.procoregov-qa.internal:5050/poc/configurations/reforge:latest
  variables:
    REFORGE_GITLAB_URL: $CI_SERVER_URL
  script:
    - reforge --config reforge.toml --dry-run
  rules:
    - if: $CI_PIPELINE_SOURCE == "schedule"
    - if: $CI_PIPELINE_SOURCE == "web"
      when: manual
```

Starts with `--dry-run` for safety. Remove once validated.

### 3c. Set CI variable: `REFORGE_TOKEN`

Via GitLab API:

```bash
curl -sk -X POST \
  -H "PRIVATE-TOKEN: <admin-or-maintainer-token>" \
  "https://gitlab.mgmt.procoregov-qa.internal/api/v4/projects/4/variables" \
  -F "key=REFORGE_TOKEN" \
  -F "value=<reforge-bot-pat>" \
  -F "masked=true" \
  -F "protected=false"
```

The reforge bot PAT (already stored in git credentials) needs at least `api` and `write_repository` scopes for creating branches and MRs.

### 3d. Create a pipeline schedule (optional)

```bash
curl -sk -X POST \
  -H "PRIVATE-TOKEN: <token>" \
  "https://gitlab.mgmt.procoregov-qa.internal/api/v4/projects/4/pipeline_schedules" \
  -F "description=Reforge dependency scan" \
  -F "ref=main" \
  -F "cron=0 6 * * 1-5" \
  -F "active=true"
```

Runs Monday–Friday at 6am UTC.

---

## Phase 4: Validate End-to-End

1. **Trigger a manual pipeline** via GitLab UI or API (`POST /projects/4/pipeline` with `ref=main`)
2. **Verify dry-run output** — reforge should discover the `helmVersion: "14.1.0"` in `apps/app/login.yaml` and any dependencies in values files, query the OCI registry, and report available updates
3. **Remove `--dry-run`** from `.gitlab-ci.yml` and trigger again to verify MR creation
4. **Verify MR quality** — check that the MR has the correct branch name (`reforge/helm-stateless-http-service-<version>`), proper diff, and descriptive body

---

## What Reforge Will Manage in This Repo

Once operational, reforge will scan for and propose updates to:

| File | Field | Registry |
|------|-------|----------|
| `apps/app/login.yaml` | `helmVersion: "14.1.0"` | `oci://oci-charts.artifacts.procoretech.com/...` |
| `values/login/values.yaml` | Image tags in `repository`+`tag` patterns | Docker/OCI registries |
| Any future `apps/**/*.yaml` | `helmVersion` fields | OCI chart registries |
| Any future `Chart.yaml` files | `dependencies[].version` | Helm repos / OCI registries |

---

## Future Work

- **CI-based reforge image builds**: Add a `.gitlab-ci.yml` to the reforge source repo that builds and pushes the container image on version tags, rather than building locally.
- **Private registry auth**: Configure `[registries."oci-charts.artifacts.procoretech.com"]` in `reforge.toml` with credentials so reforge can query the Procore OCI registry for available chart versions.
- **Additional apps**: Add more app definitions under `apps/app/` and `apps/tooling/` as services are onboarded.
- **Reforge custom manager for app definitions**: The `helmVersion` field in the app YAML files is a custom format (not standard `Chart.yaml`). Reforge will need a regex manager (see `todo/06-regex-manager.md`) to detect these version strings. Until then, only standard Helm `Chart.yaml` dependencies and `values.yaml` image tags are auto-detected.
