# Managers

Managers are the components that detect dependency references in your files and determine how to look up newer versions. Reforge ships with three managers: **Helm**, **Docker**, and **Regex** (custom).

## Helm Manager

The Helm manager scans two file types: `Chart.yaml` (subchart dependencies) and `values.yaml` (image tags).

### Chart.yaml

Reforge reads the `dependencies` list and resolves each entry against its declared repository.

```yaml
# Chart.yaml
dependencies:
  - name: redis
    version: "18.6.1"
    repository: "https://charts.bitnami.com/bitnami"

  - name: my-service
    version: "2.3.0"
    repository: "oci://artifactory.example.com/helm-charts"

  - name: ingress-nginx
    version: "4.9.0"
    repository: "@ingress"   # alias supported
```

Supported repository formats:

| Format | Example |
|--------|---------|
| HTTP/HTTPS Helm repo | `https://charts.bitnami.com/bitnami` |
| OCI registry | `oci://registry.example.com/charts` |
| Alias (`@name` or `alias:name`) | `@ingress` |

### values.yaml

For images referenced in `values.yaml`, reforge looks for `repository` + `tag` sibling pairs:

```yaml
# values.yaml
image:
  repository: nginx
  tag: "1.27.0"      # reforge will propose a tag update here

sidecar:
  image:
    repository: curlimages/curl
    tag: "8.12.1"
```

The `repository` value is treated as a Docker image name. The `tag` field is updated in-place, preserving all YAML comments and surrounding formatting.

### Lock File Maintenance

When `Chart.yaml` dependencies are updated, reforge also updates `Chart.lock` if it exists. SHA256 digests are computed automatically. Optionally point reforge at a `helm` binary for digest generation:

```toml
[lockfile]
enabled     = true
helm_binary = "/usr/local/bin/helm"
```

See [Advanced Features — Lock Files](advanced.md#lock-file-maintenance).

---

## Docker Manager

The Docker manager handles two file types: `Dockerfile` and `docker-compose.yml` / `docker-compose.yaml`.

### Dockerfile

Every `FROM` line is detected and updated:

```dockerfile
# Simple image
FROM nginx:1.27.0

# Multi-platform
FROM --platform=linux/amd64 ubuntu:22.04

# Multi-stage
FROM golang:1.22 AS builder
FROM alpine:3.19

# Registry with port
FROM registry.example.com:5000/my-app:1.2.3

# ARG-based reference
ARG BASE_IMAGE=nginx:1.27.0
FROM ${BASE_IMAGE}
```

> **ARG-based images:** When a `FROM` line references an `ARG`, reforge traces the `ARG` definition and updates the version there.

### Docker Compose

Service `image` fields are detected and updated:

```yaml
# docker-compose.yml
services:
  web:
    image: nginx:1.27.0

  db:
    image: postgres:16.2

  cache:
    image: registry.example.com/redis:7.2.4
```

---

## Regex Manager

The Regex manager lets you extract dependency names and versions from arbitrary files using named capture groups. This is useful for custom version files, scripts, or config formats that the Helm and Docker managers don't cover.

### Configuration

Add one or more `[[regex_managers]]` entries to `reforge.toml`:

```toml
[[regex_managers]]
name          = "helm-chart-version"
file_patterns = ["infrastructure/**.yaml"]
match_pattern = 'helmChart:\s+"(?P<depName>[^"]+)"\s+helmVersion:\s+"(?P<currentValue>[^"]+)"'
datasource    = "helm-repo"
registry_url  = "https://charts.example.com"

[[regex_managers]]
name          = "app-image-versions"
file_patterns = ["versions.env"]
match_pattern = 'APP_IMAGE_TAG=(?P<depName>myapp):(?P<currentValue>[\d.]+)'
datasource    = "docker"
```

### Named Capture Groups

| Group | Required | Description |
|-------|----------|-------------|
| `depName` | **yes** | Dependency name (image name or chart name) |
| `currentValue` | **yes** | Current version string |
| `registryUrl` | no | Registry or repo URL (overrides `registry_url` config key) |
| `datasource` | no | Datasource override per-match |

### Datasources

| Value | Looks up versions via |
|-------|-----------------------|
| `docker` | Docker/OCI Registry v2 API |
| `helm-oci` | OCI Helm registry |
| `helm-repo` | HTTP/HTTPS Helm `index.yaml` |

### Validation

Reforge validates all `[[regex_managers]]` entries at startup:

- `match_pattern` must be a valid regex.
- `depName` and `currentValue` capture groups must be present.
- `datasource` must be one of the three supported values.

Invalid entries cause reforge to exit with a config error before scanning begins.

---

## Registry Authentication

All managers share the same registry credential configuration. See [Configuration — `[registries]`](configuration.md#registrieshost).

### Docker Hub

No credentials required for public images. For private Docker Hub images:

```toml
[registries."registry-1.docker.io"]
username     = "my-dockerhub-username"
password_env = "DOCKERHUB_TOKEN"
```

### Artifactory OCI

```toml
[registries."artifactory.example.com"]
password_env = "ARTIFACTORY_API_KEY"   # sent as Bearer token when username is omitted
```

### Private Helm Repositories

Registry credentials are also used when fetching `index.yaml` from password-protected HTTP Helm repos.
