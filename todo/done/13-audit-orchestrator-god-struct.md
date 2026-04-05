# [MEDIUM] Break up the Orchestrator god struct

## Priority: Medium
## Status: Done

## Description

`Orchestrator` in `src/orchestrator.rs` has 14 fields and ~1300 lines of code handling unrelated concerns: HTTP client management, registry lookups, file scanning, MR creation, changelog fetching, vulnerability checking, replacement detection, dashboard generation, branch naming, and output formatting.

## File
`src/orchestrator.rs` — entire file

## Category
Anti-Patterns / Structure and Modularity (Audit Checklist §9, §10)

## Issue
The struct accumulates all application state and orchestration logic into one type, making it difficult to test individual behaviors, reason about data flow, or modify one concern without risk of breaking others. Several methods (`process_with_source`, `apply_local_updates`, `create_gitlab_mrs`, `handle_replacement_actions`) exceed 100 lines and interleave multiple responsibilities.

## Suggested fix direction
Extract cohesive groups of functionality into their own types:
1. **`RegistryLookup`** — wraps `docker_registry`, `helm_registry`, `http_client`; owns `check_for_update()` and `fetch_dep_digest()`.
2. **`MrBuilder`** — owns `build_group_mr_content()`, branch naming, and MR body rendering.
3. **`Scanner`** — owns file pattern matching logic (`matches_any_pattern`, `file_matches_manager`).
4. Move `deduplicate_candidates`, `print_json_report`, `print_dry_run_report` to standalone functions or a `Reporter` struct.

The `Orchestrator` should remain as a thin coordinator that delegates to these components.

## References
- Rust Design Patterns Anti-Pattern: God struct
- Effective Rust Item 22: Minimize visibility

## Acceptance Criteria

- [x] `Orchestrator` has ≤8 fields
- [x] No method on `Orchestrator` exceeds 60 lines
- [x] Extracted types are independently unit-testable
- [x] `cargo test` passes with no regressions
