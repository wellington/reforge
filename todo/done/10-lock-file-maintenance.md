# Lock File Maintenance

Regenerate lock files when dependency manifests are updated.

## Why
- Updating `Chart.yaml` without updating `Chart.lock` leaves the lock file stale and Helm will refuse to install
- Lock files ensure reproducible builds; stale locks break CI

## What's needed
- **Helm Chart.lock**: After updating `Chart.yaml` dependencies, regenerate `Chart.lock` with correct digests and versions
- **Lock file format**: Parse and produce the `Chart.lock` YAML format (dependencies with name, version, repository, digest)
- **Digest fetching**: For each dependency, fetch the chart archive digest from the registry to populate the lock file
- **Local git mode dependency**: If using API-only mode, generate the lock file content and commit it alongside `Chart.yaml`. If using local git mode (todo/01), can shell out to `helm dependency update`.
- **Compose lock files**: Handle `docker-compose` lock files if/when the format stabilizes

## Estimated scope
~300-400 lines. Chart.lock parsing/generation, digest fetching from registries, coordinated multi-file commits.
