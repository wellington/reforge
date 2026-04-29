# Project Architecture

**Last updated:** 2026-04-04
**Type:** Observation
**Status:** Active

## Description

Reforge is a Rust reimplementation of Renovate's core functionality, scoped to Helm charts and Dockerfiles/Docker Compose, targeting self-managed GitLab as the platform backend. It supports both GitLab API mode and local git mode.

## Context

Applies to all development on this project. Understanding the module layout is essential before modifying any component.

## Details

### Module Layout

- `main.rs` — CLI entry point using `clap` derive macros. Layers config from CLI args > env vars > TOML file.
- `config.rs` — `Config` struct deserialized from TOML. Supports registry credentials via env var indirection (`password_env` field), `base_url` overrides, and `insecure` TLS mode.
- `error.rs` — `thiserror`-based `ReforgeError` enum with variants for GitLab API, registry, parsing, config, git CLI, and passthrough errors.
- `platform/gitlab.rs` — `GitLabClient` wrapping `reqwest::Client`. Handles tree listing, file fetching, branch/commit/MR/issue management. Supports `insecure` TLS for self-signed certs.
- `platform/git.rs` — `GitRepo` wrapping local `git` CLI via `tokio::process::Command`. Supports branch operations, commits, rebase, conflict detection.
- `platform/mod.rs` — `FileSource` trait abstracting over `GitLabSource` and `LocalGitSource`.
- `manager/mod.rs` — `PackageManager` trait and shared types (`Dependency`, `RegistrySource`, `UpdateContext`).
- `manager/docker.rs` — Parses `FROM` lines (with `--platform`, `AS`, multi-stage, ARG-based refs, registry ports) and Docker Compose `services.*.image` values.
- `manager/helm.rs` — Parses `Chart.yaml` dependencies (HTTP, OCI, alias repos) and walks `values.yaml` for `repository`+`tag` sibling patterns.
- `manager/regex.rs` — User-defined regex patterns for extracting versions from arbitrary files. Named capture groups (`depName`, `currentValue`, `registryUrl`, `datasource`).
- `registry/mod.rs` — `RegistryClient` trait and `VersionInfo` type. Includes lenient version parser that handles `v` prefixes and two-part versions.
- `registry/docker.rs` — Docker/OCI Registry v2 client with token auth flow (Bearer + Basic), pagination, `base_url` overrides, and API-key-as-Bearer pre-auth for Artifactory.
- `registry/helm.rs` — Fetches `index.yaml` from Helm repos. Delegates OCI Helm charts to the Docker registry client.
- `versioning.rs` — `VersionPolicy` with `SemverPatch`, `SemverMinor`, `SemverMajor` strategies. Filters prereleases.
- `updater.rs` — String-based file updates (not AST round-trip) to preserve YAML comments and formatting. Supports Dockerfile, YAML key path, Docker Compose, regex match, and replacement contexts.
- `orchestrator.rs` — Top-level workflow: scan project tree → extract deps → check replacements → fetch versions concurrently (buffered to 5) → group candidates → create branches/commits/MRs → write dashboard. Handles both GitLab API and local git modes.
- `dashboard.rs` — Generates Dependency Dashboard as a GitLab issue or local Markdown file with update status table.
- `automerge.rs` — Policy-based automerge evaluation by dependency name glob, update type, and minimum age.
- `scheduling.rs` — Rate limiting (max open MRs), schedule windows (day/hour restrictions), priority ordering.
- `grouping.rs` — Groups update candidates by configurable rules (update type, manager, pattern, path).
- `changelog.rs` — Fetches release notes from GitHub Releases API; renders as collapsible MR section.
- `vulnerability.rs` — Queries OSV API for known CVEs; adds security labels and vulnerability details to MRs.
- `rebase.rs` — Detects stale MRs (behind default branch or conflicting); applies rebase/recreate/ignore strategy.
- `lockfile.rs` — Parses and updates `Chart.lock` files alongside `Chart.yaml` changes; computes SHA256 digests.
- `replacement.rs` — Detects deprecated/renamed images/charts using built-in and TOML-defined rules; creates migration MRs.

### Key Design Decisions

1. **String-based file updates** — YAML round-trip libraries lose comments and reorder keys. Targeted string replacement preserves formatting.
2. **One MR per dependency by default** — Matches Renovate's default. Grouping is configurable via `grouping_rules`.
3. **Async from the start** — `tokio` + `reqwest` with `futures::stream::buffered()` for concurrent registry lookups.
4. **Dual mode operation** — Works through GitLab API (for CI/CD pipelines) or local git CLI (for development/testing).
5. **Config file is optional** — Everything can be driven by CLI flags and env vars.
6. **Registry credential overrides** — `base_url` field on credentials allows redirecting registries to mock servers or proxies. API-key-as-Bearer pattern supports Artifactory OCI registries.
7. **TLS insecure mode** — `gitlab.insecure = true` for self-signed GitLab instances.

### Key Dependencies

`clap` (CLI), `tokio` (async), `reqwest` (HTTP), `serde`/`serde_yaml`/`serde_json` (serialization), `semver` (version comparison), `regex` (Dockerfile/custom parsing), `tracing` (logging), `thiserror`/`anyhow` (errors), `toml` (config), `base64` (registry auth), `chrono` (scheduling), `sha2` (Chart.lock digests), `async-trait` (FileSource abstraction), `futures` (concurrent streams).

### Testing

- **Unit tests:** 161 across all modules
- **Integration tests:** 6 end-to-end tests in `tests/end_to_end.rs` using `wiremock` for mock Docker/OCI registries and `tempfile` for ephemeral git repos
- **Dev dependencies:** `wiremock`, `tempfile`, `serde_json`

## Summary

The codebase is ~15,000 lines of Rust across 20 source files (plus 592 lines of integration tests) with 167 total tests. The project is named **reforge** (see `project-naming.md`). All 11 planned features from the original `todo/` backlog are implemented and verified.

## Notes
- Project paths are URL-encoded with `%2F` for GitLab API calls — custom `urlencoding` module in `platform/gitlab.rs`.
- Docker Hub images without a registry prefix resolve to `registry-1.docker.io` and library images (e.g., `nginx`) resolve to `library/nginx`.
- 36 compiler warnings remain (dead code for API surface not yet wired, unused struct fields for future features). None are errors.
