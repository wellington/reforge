# [HIGH] Replace `expect()` / `unwrap()` calls in production code with proper error handling

## Priority: High
## Status: Done

## Description

Several non-test code paths use `.expect()` which will panic at runtime on failure. These should return `Result` and propagate errors to the caller.

## Locations

- `src/orchestrator.rs:75` — `reqwest::Client::builder().build().expect("Failed to build HTTP client")`
- `src/orchestrator.rs:154` — `self.gitlab.as_ref().expect("GitLab client required in API mode")`
- `src/orchestrator.rs:517` — `self.gitlab.as_ref().expect("GitLab client required")`
- `src/registry/docker.rs:34` — `reqwest::Client::builder().build().expect("Failed to build HTTP client")`
- `src/registry/helm.rs:35` — `reqwest::Client::builder().build().expect("Failed to build HTTP client")`

## Category
Error Handling (Audit Checklist §1)

## Issue
`expect()` in production code causes the entire process to abort with a panic if the condition fails. While HTTP client construction rarely fails, the GitLab `expect` calls at lines 154 and 517 can be triggered by misconfiguration (running in local mode but hitting a code path that requires GitLab). These should return a `ReforgeError::Config` instead of panicking.

## Suggested fix direction
- For HTTP client builders: return `Result` from `DockerRegistryClient::new()` and `HelmRegistryClient::new()` instead of panicking. Propagate with `?`.
- For `self.gitlab.as_ref().expect(...)`: replace with `.ok_or_else(|| ReforgeError::Config("GitLab client required in API mode".into()))?`.

## References
- Effective Rust Item 18: Don't panic
- Clippy lint `clippy::expect_used` (restriction category)

## Acceptance Criteria

- [x] No `.expect()` calls remain in `src/` outside of `#[cfg(test)]` modules and `LazyLock` static regex initialization
- [x] `DockerRegistryClient::new()` and `HelmRegistryClient::new()` return `Result`
- [x] `cargo check` and `cargo test` pass
