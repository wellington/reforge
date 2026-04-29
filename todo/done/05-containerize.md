# Containerize Reforge

## Priority: Medium
## Status: Open

## Description

Reforge should be packaged as a container image so it can run as a scheduled job in Kubernetes/ArgoCD or GitLab CI. The deployment-plan.md outlines deploying it via a CronJob that scans the `configurations` repo on a schedule.

## Details

- Multi-stage Dockerfile: build with `rust:1-slim` -> runtime with `debian:bookworm-slim` (or `distroless`)
- Include `git` binary in runtime image for local git mode
- Publish to an OCI registry (GHCR, Artifactory, etc.)
- Helm chart or raw K8s manifests for CronJob deployment
- Environment variables for `REFORGE_TOKEN`, `ARTIFACTORY_API_KEY`
- Mount `reforge.toml` via ConfigMap

## Acceptance Criteria

- [ ] Dockerfile builds a static or dynamically-linked binary
- [ ] Image size < 100MB
- [ ] `docker run` with env vars executes a scan
- [ ] K8s CronJob manifest or Helm chart for scheduled execution
