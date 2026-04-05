# Rust Code Audit — reforge

**Date:** 2026-04-05
**Auditor:** Rust Code Auditor (automated)
**Files audited:** 26 (20 source + 1 integration test + Cargo.toml)
**Findings:** 0 critical, 3 high, 7 medium, 5 low

## High Findings

| # | File | Title | TODO |
|---|------|-------|------|
| H1 | `orchestrator.rs`, `registry/docker.rs`, `registry/helm.rs` | `expect()` in production code paths will panic on failure | `todo/10-audit-expect-in-production.md` |
| H2 | `automerge.rs`, `grouping.rs`, `dashboard.rs`, `manager/docker.rs`, `manager/helm.rs` | Duplicated utility functions (`glob_match`, `manager_name`, `parse_image_reference`) across 5+ files | `todo/11-audit-duplicated-code.md` |
| H3 | `config.rs`, `platform/gitlab.rs`, `manager/regex.rs` | Stringly-typed fields where enums should be used (`action`, `pin_strategy`, `datasource`, `grouping`) | `todo/12-audit-stringly-typed-apis.md` |

## Medium Findings

| # | File | Title |
|---|------|-------|
| M1 | 10 files | 32 `#[allow(dead_code)]` annotations — audit and clean up |
| M2 | `manager/mod.rs`, `registry/mod.rs`, `orchestrator.rs` | Core types missing `PartialEq`/`Eq` derivations |
| M3 | `orchestrator.rs:235` | Unnecessary `.clone()` of entire dependency vector |
| M4 | `orchestrator.rs:1283` | `HashSet` double-lookup instead of entry API |
| M5 | `updater.rs:39` | `FileUpdate.original_content` stored but never read |
| M6 | `config.rs`, `replacement.rs`, `dashboard.rs` | Blocking `std::fs` I/O in async context |
| M7 | `platform/gitlab.rs:121` | `request_with_retry` doesn't retry — misleading name |

Medium and low findings are grouped in `todo/14-audit-medium-low-findings.md`.

The `Orchestrator` god struct finding has its own file at `todo/13-audit-orchestrator-god-struct.md`.

## Low Findings

| # | File | Title |
|---|------|-------|
| L1 | `orchestrator.rs:165` | `let _ = gitlab;` no-op borrow extension |
| L2 | All files | No public doc comments on types/functions |
| L3 | `platform/gitlab.rs`, `orchestrator.rs` | Magic numbers without named constants |
| L4 | `platform/git.rs:9` | `GitRepo.path` unnecessarily `pub` |
| L5 | Throughout | Inconsistent error context patterns |

## Positive Observations

- **Error types are well-designed.** `ReforgeError` uses `thiserror` with meaningful variants — exactly as recommended.
- **Async architecture is solid.** Proper use of `tokio`, `futures::stream::buffered()`, and `async-trait` for the `FileSource` abstraction.
- **Test coverage is strong.** 167 tests covering happy paths and edge cases across all modules.
- **String-based file updates** is a thoughtful design decision that preserves YAML comments and formatting.
- **Retry logic with exponential backoff** in `GitLabClient` is well-implemented.
- **Config layering** (CLI > env > TOML) with RENOVATE_ fallbacks is clean and user-friendly.

## How to Work Through These Findings

TODO files are numbered `10-14` and should be worked in order:

1. **`10-audit-expect-in-production.md`** — Quick, high-impact safety fix
2. **`11-audit-duplicated-code.md`** — Extract shared utilities
3. **`12-audit-stringly-typed-apis.md`** — Replace strings with enums
4. **`13-audit-orchestrator-god-struct.md`** — Structural refactor (larger effort)
5. **`14-audit-medium-low-findings.md`** — Individual items, work through the list

Each file contains acceptance criteria that should be verified with `cargo check` and `cargo test`.
