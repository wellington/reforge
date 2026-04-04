# Dead Code Cleanup

## Priority: Low
## Status: Open

## Description

There are 36 compiler warnings (all `dead_code` / `unused` variants). These are for struct fields, methods, and functions that were built as API surface but aren't yet wired into the orchestrator or are reserved for future use.

## Details

Key areas with dead code:
- `platform/git.rs` — `clone()`, `commit_all()`, `push()`, `status()`, `log()`, `LogEntry`
- `platform/gitlab.rs` — `UpdateMrParams`, `request_with_retry()`, `delete_branch()`, `update_mr()`, `close_mr()`, `merge_mr()`, `accept_mr()`, various struct fields
- `scheduling.rs` — `PriorityOrder`, `sort_candidates_by_priority()`, `sort_candidates_by_priority_with_security()`
- `rebase.rs` — `recreate_mr_branch()`, `StaleMr` fields
- `lockfile.rs` — `ChartLock`, `ChartLockDependency`, `parse_chart_lock()`, `generate_chart_lock()`
- `config.rs` — `helm_binary`, `versioning`, `priority_boost` fields
- `manager/mod.rs` — `UpdateContext` variant fields (`keys`, `full_reference`, `service_path`)
- `updater.rs` — `FileUpdate.original_content`
- `vulnerability.rs` — `OsvEvent.introduced`, `VulnerabilityChecker::with_url()`, `is_security_update()`

## Acceptance Criteria

- [ ] Either wire the dead code into the orchestrator or suppress with `#[allow(dead_code)]` where it's intentional API surface
- [ ] 0 compiler warnings on `cargo check`
