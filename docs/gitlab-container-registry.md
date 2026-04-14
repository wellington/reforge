# Enable Container Registry on GitLab

## Instance

`gitlab.mgmt.procoregov-qa.internal` (GitLab CE 18.10.0, Helm-based deployment)

## Current State

- The container registry is **not configured** at the instance level
- Project-level registry toggles are enabled but non-functional (no backing service)
- Docker login to the instance returns `404: dependency proxy not enabled` on `/v2/`
- The instance uses a self-signed TLS certificate chain issued by `Procore Technologies Root CA` → `GovCloud Staging Issuing CA`

## Desired State

Enable the GitLab integrated container registry so projects can push and pull container images.

### Helm Chart Values

```yaml
registry:
  enabled: true

global:
  registry:
    enabled: true
    # Option A: registry on a subdomain (preferred, avoids port conflicts)
    host: registry.mgmt.procoregov-qa.internal
    # Option B: registry on the same domain with a different port
    # host: gitlab.mgmt.procoregov-qa.internal
    # port: 5050
```

If using Option A (subdomain), a DNS record for `registry.mgmt.procoregov-qa.internal` pointing to the same ingress is required, along with a TLS certificate covering that hostname (or a wildcard cert for `*.mgmt.procoregov-qa.internal`).

If using Option B (port-based), the ingress or load balancer must expose port 5050 and route it to the registry service.

### TLS

The registry endpoint must be covered by a certificate signed by the same CA chain (`Procore Technologies Root CA`) so that Docker clients with the CA installed can push/pull without `--insecure-registry`.

### Storage

The registry needs a storage backend. Options:
- **S3-compatible** (recommended for production): MinIO, AWS S3, or any S3-compatible store
- **Filesystem**: PVC-backed storage on the cluster (simpler but less durable)

```yaml
registry:
  storage:
    # S3 example
    s3:
      bucket: gitlab-registry
      region: us-east-1
      # accesskey/secretkey via secret reference
    # Or filesystem
    # filesystem:
    #   rootdirectory: /var/lib/registry
```

### Verification

After enabling, the following should succeed:

```bash
# Registry endpoint responds
curl -sk https://<registry-host>/v2/
# Returns 401 (auth required) — this is correct

# Docker login works
docker login <registry-host> -u oauth2 -p <gitlab-pat>
# Returns "Login Succeeded"

# Push works
docker push <registry-host>/poc/configurations/reforge:latest
```

## Why

Reforge is built as a container image and needs to be published to a registry so it can run as a scheduled CI job in `poc/configurations`. The CI pipeline pulls the reforge image to scan for outdated dependencies and open MRs. Without a registry, the image must be transferred manually.
