# [MEDIUM/LOW] Grouped audit findings — code quality improvements

## Priority: Medium/Low
## Status: Done (moved to todo/done/)

## Description

Collected medium and low severity findings from the Rust code audit. Each item is independently fixable. Address in priority order within the file.

---

## MEDIUM Findings

### M1. Excessive `#[allow(dead_code)]` (32 instances across 10 files)

**Files:** `config.rs`, `platform/gitlab.rs`, `platform/git.rs`, `manager/mod.rs`, `lockfile.rs`, `scheduling.rs`, `rebase.rs`, `updater.rs`, `registry/helm.rs`, `vulnerability.rs`
**Category:** Structure and Modularity (§9)
**Issue:** 32 `#[allow(dead_code)]` annotations suppress warnings for significant public API surface. Several are on fields/methods that appear genuinely unused (e.g., `VulnerabilityConfig.priority_boost`, `LogEntry`, various `pub` methods on `GitLabClient`). Dead code should either be removed or, if it's planned API surface, marked with a comment explaining the intent.
**Suggested fix direction:** Audit each `#[allow(dead_code)]` — remove truly dead code, remove the annotation where the code is actually used (may have been added prematurely), or add a comment like `// Used by future CI integration` on intentional reserves.

### M2. Missing standard trait derivations

**Files:** `src/manager/mod.rs` (`Dependency`, `RegistrySource`, `UpdateContext`), `src/registry/mod.rs` (`VersionInfo`), `src/orchestrator.rs` (`UpdateCandidate`), `src/platform/mod.rs` (`FileEntry`)
**Category:** Type System (§3)
**Issue:** Core data types lack `PartialEq`/`Eq` derivations, making them harder to use in tests and assertions. `RegistrySource` and `UpdateContext` are enums that should derive at least `PartialEq`. `FileEntry` should derive `PartialEq, Eq, Hash`.
**Suggested fix direction:** Add `#[derive(PartialEq, Eq)]` where applicable. For types containing `String` fields, `PartialEq` is free. For `VersionInfo` containing `semver::Version`, check that `Version` implements `PartialEq` (it does).

### M3. Unnecessary cloning of large vectors

**File:** `src/orchestrator.rs:235`
**Category:** Performance (§6)
**Issue:** `stream::iter(all_deps.clone())` clones the entire dependency list (including file contents) just to iterate it concurrently. The `all_deps` vector is used later only for replacement checking and dashboard building, which could use references.
**Suggested fix direction:** Restructure to avoid the clone — e.g., drain `all_deps` into the stream and rebuild the flat reference list from groups, or use `Arc<Vec<...>>` for shared ownership.

### M4. `HashMap` entry API not used

**File:** `src/orchestrator.rs:1283-1303` (`deduplicate_candidates`)
**Category:** Performance (§6)
**Issue:** The function uses `seen.contains(&key)` followed by `seen.insert(key)` — a double lookup. The `HashSet::insert()` method already returns `false` if the element was present.
**Suggested fix direction:** Replace with `if !seen.insert(key) { continue; }`.

### M5. `FileUpdate` stores `original_content` unnecessarily

**File:** `src/updater.rs:39-45`
**Category:** Performance (§6)
**Issue:** `FileUpdate` stores both `original_content` and `updated_content`. The `original_content` field is never read after construction — it's marked `#[allow(dead_code)]`. For large files this doubles memory usage.
**Suggested fix direction:** Remove `original_content` from `FileUpdate`. If it's needed for debugging, log it before constructing the struct or add it back behind a feature flag.

### M6. Blocking I/O called from async context

**Files:** `src/config.rs:496` (`Config::load`), `src/replacement.rs:92` (`ReplacementDatabase::load_from_toml`), `src/dashboard.rs:242` (`write_local_dashboard`)
**Category:** Async (§7)
**Issue:** `std::fs::read_to_string()` and `std::fs::write()` block the tokio runtime thread. While these only read config files (small and infrequent), it's an anti-pattern that may cause issues if the config is on a network filesystem or if the call frequency increases.
**Suggested fix direction:** Use `tokio::fs::read_to_string()` and `tokio::fs::write()`, making `Config::load` and `ReplacementDatabase::load_from_toml` async. Alternatively, wrap in `tokio::task::spawn_blocking()`.

### M7. `request_with_retry` is misleadingly named

**File:** `src/platform/gitlab.rs:121-130`
**Category:** API Design (§4)
**Issue:** `request_with_retry()` doesn't retry anything — it just builds a `RequestBuilder`. The actual retry logic is in `send_with_retry()`. The name suggests retry behavior that doesn't exist.
**Suggested fix direction:** Rename to `build_request()` or inline it since it's a trivial wrapper.

---

## LOW Findings

### L1. `let _ = gitlab;` is a no-op borrow extension

**File:** `src/orchestrator.rs:165`
**Category:** Style
**Issue:** The comment says "ensure borrow extends to here" but `gitlab` is `Option<GitLabClient>`, not a reference. This binding has no effect.
**Suggested fix direction:** Remove the line.

### L2. No public doc comments

**Files:** All source files
**Category:** Documentation (§11)
**Issue:** Almost no public types or functions have `///` doc comments. While this is an internal tool, doc comments improve discoverability and serve as inline documentation for future contributors.
**Suggested fix direction:** Add doc comments to at least: `ReforgeError`, `Config`, `Orchestrator::new()`, `Orchestrator::run()`, `FileSource` trait methods, `PackageManager` trait methods, `RegistryClient` trait methods.

### L3. Magic numbers without named constants

**File:** `src/platform/gitlab.rs:158` (backoff formula `500 * 2u64.pow(attempt)`), `src/orchestrator.rs:30` (`CONCURRENCY_LIMIT = 5`), various `per_page = 100`
**Category:** Structure and Modularity (§9)
**Issue:** Backoff timing, page sizes, and concurrency limits are hardcoded literals scattered through the code.
**Suggested fix direction:** Extract to named constants: `RETRY_BASE_DELAY_MS`, `GITLAB_PER_PAGE`, etc. `CONCURRENCY_LIMIT` is already a constant — apply the same treatment to the others.

### L4. `GitRepo.path` is unnecessarily `pub`

**File:** `src/platform/git.rs:9`
**Category:** Structure and Modularity (§9)
**Issue:** `GitRepo.path` is `pub` but is only accessed outside the struct in `orchestrator.rs` via `self.config.local_path` (not via `repo.path`). The field should be `pub(crate)` at most.
**Suggested fix direction:** Change to `pub(crate)` or provide a getter method.

### L5. Inconsistent error context patterns

**Files:** Throughout codebase
**Category:** Error Handling (§1)
**Issue:** Error context is added inconsistently — some use `map_err(|e| ReforgeError::X(format!("...: {}", e)))`, others use `?` with `#[from]` conversion. Neither pattern is wrong but the inconsistency makes it harder to trace errors to their source.
**Suggested fix direction:** Standardize on using `map_err()` to add file/operation context wherever a `?` would lose important information (file paths, registry names, etc.).

## Acceptance Criteria

- [ ] Each finding addressed or explicitly deferred with justification
- [ ] `cargo check` and `cargo test` pass after all changes
- [ ] No new warnings introduced
