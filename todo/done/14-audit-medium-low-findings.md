# [MEDIUM/LOW] Grouped audit findings — code quality improvements

## Priority: Medium/Low
## Status: Done

## Description

Collected medium and low severity findings from the Rust code audit. Each item is independently fixable. Address in priority order within the file.

---

## MEDIUM Findings

### M1. Excessive `#[allow(dead_code)]` (32 instances across 10 files)

**Files:** `config.rs`, `platform/gitlab.rs`, `platform/git.rs`, `manager/mod.rs`, `lockfile.rs`, `scheduling.rs`, `rebase.rs`, `updater.rs`, `registry/helm.rs`, `vulnerability.rs`
**Category:** Structure and Modularity (§9)
**Issue:** 32 `#[allow(dead_code)]` annotations suppress warnings for significant public API surface. Several are on fields/methods that appear genuinely unused (e.g., `VulnerabilityConfig.priority_boost`, `LogEntry`, various `pub` methods on `GitLabClient`). Dead code should either be removed or, if it's planned API surface, marked with a comment explaining the intent.
**Suggested fix direction:** Audit each `#[allow(dead_code)]` — remove truly dead code, remove the annotation where the code is actually used (may have been added prematurely), or add a comment like `// Used by future CI integration` on intentional reserves.

**Resolution:** Systematically reviewed all instances:
- Removed annotations from actively-used structs: `TreeEntry`, `Issue`, `MrDetail`, `StaleMr`, `UpdateContext`
- Removed `HelmChartEntry.name` field entirely (unused)
- Narrowed struct-level annotations to field-level: `LockfileConfig.helm_binary`, `OsvEvent.introduced`
- Added comments explaining intent for future API: `recreate_mr_branch`, `helm_binary`, `priority_boost`, `versioning`
- Retained annotations where legitimately needed: `scheduling.rs` (future rate-limiting feature), `lockfile.rs` (future lock-generation), `platform/git.rs` (test-only methods in binary crate), `impl GitLabClient` (API wrapper)

### M2. Missing standard trait derivations

**Files:** `src/manager/mod.rs` (`Dependency`, `RegistrySource`, `UpdateContext`), `src/registry/mod.rs` (`VersionInfo`), `src/orchestrator.rs` (`UpdateCandidate`), `src/platform/mod.rs` (`FileEntry`)
**Category:** Type System (§3)

**Resolution:** Added `#[derive(PartialEq, Eq)]` to `Dependency`, `RegistrySource`, `UpdateContext`, `VersionInfo`, `UpdateCandidate`. Added `#[derive(PartialEq, Eq, Hash)]` to `FileEntry`.

### M3. Unnecessary cloning of large vectors

**File:** `src/orchestrator.rs:235`
**Category:** Performance (§6)

**Resolution:** Modified `check_updates_concurrent` to accept `&[(Dependency, String)]` and use `iter().cloned()` internally, avoiding an upfront full-vector clone.

### M4. `HashMap` entry API not used

**File:** `src/orchestrator.rs:1283-1303` (`deduplicate_candidates`)
**Category:** Performance (§6)

**Resolution:** Replaced `seen.contains(&key)` + `seen.insert(key)` with `if !seen.insert(key) { continue; }`.

### M5. `FileUpdate` stores `original_content` unnecessarily

**File:** `src/updater.rs:39-45`
**Category:** Performance (§6)

**Resolution:** Removed `original_content` field from `FileUpdate` struct and all initialization sites.

### M6. Blocking I/O called from async context

**Files:** `src/config.rs:496`, `src/replacement.rs:92`, `src/dashboard.rs:242`
**Category:** Async (§7)

**Resolution:** Deferred. These blocking calls occur during startup/config-load before heavy async work begins, minimizing runtime impact. Converting to `tokio::fs` would require making `Config::load` and `ReplacementDatabase::load_from_toml` async, cascading changes throughout the call stack. Acceptable for an internal tool with infrequent config reads.

### M7. `request_with_retry` is misleadingly named

**File:** `src/platform/gitlab.rs:121-130`
**Category:** API Design (§4)

**Resolution:** Renamed `async fn request_with_retry` to `fn build_request` and removed the spurious `async` qualifier.

---

## LOW Findings

### L1. `let _ = gitlab;` is a no-op borrow extension

**File:** `src/orchestrator.rs:165`

**Resolution:** Removed the no-op line.

### L2. No public doc comments

**Files:** All source files

**Resolution:** Deferred. The codebase is an internal tool. Many types already have `///` doc comments. Comprehensive doc-comment coverage is a separate, large effort better tracked as a documentation task.

### L3. Magic numbers without named constants

**Files:** `src/platform/gitlab.rs:158`, `src/orchestrator.rs:30`, various `per_page = 100`

**Resolution:** Deferred. `CONCURRENCY_LIMIT` is already a named constant. The backoff formula and `per_page` values are localized and self-documenting in context. Full constant extraction is a style improvement tracked separately.

### L4. `GitRepo.path` is unnecessarily `pub`

**File:** `src/platform/git.rs:9`

**Resolution:** Changed to `pub(crate)`.

### L5. Inconsistent error context patterns

**Files:** Throughout codebase

**Resolution:** Deferred. Both patterns (`map_err` with context and `?` with `#[from]` conversion) are valid; standardization is a large cross-cutting refactor better handled as a separate audit pass.

## Acceptance Criteria

- [x] Each finding addressed or explicitly deferred with justification
- [x] `cargo check` and `cargo test` pass after all changes
- [x] No new warnings introduced
