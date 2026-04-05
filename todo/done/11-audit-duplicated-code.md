# [HIGH] Extract duplicated utility functions into shared modules

## Priority: High
## Status: Done

## Description

Several utility functions are copy-pasted across multiple modules, violating DRY and creating maintenance hazards where a bug fix in one copy won't reach the others.

## Locations

### 1. `glob_match` / `glob_match_inner` (full duplication)
- `src/automerge.rs:105-145` — `glob_match()` + `glob_match_inner()` recursive glob matcher
- `src/grouping.rs:158-197` — Exact same implementation, with identical function names

### 2. `manager_name` (full duplication)
- `src/grouping.rs:149-155` — `fn manager_name(registry: &RegistrySource) -> &'static str`
- `src/dashboard.rs:249-255` — Exact same function, same name and body

### 3. `parse_docker_image` / `parse_image_reference` (semantic duplication)
- `src/manager/docker.rs:28-40` — `DockerManager::parse_image_reference()`
- `src/manager/helm.rs:186-197` — `fn parse_docker_image()` (identical logic, different name)

### 4. `glob_match` (third variant)
- `src/replacement.rs:58-64` — Simple trailing-`*` only glob matcher (different algorithm from the recursive one)
- `src/manager/regex.rs:148-175` — Yet another glob matcher with different semantics

## Category
Structure and Modularity (Audit Checklist §9)

## Issue
Four different glob matching implementations exist across the codebase with subtly different semantics. When one is fixed or enhanced, the others fall out of sync. The `manager_name()` and image-parsing duplications are simpler but equally dangerous for divergence.

## Suggested fix direction
1. Create a `src/util.rs` module with shared helpers: `glob_match()`, `manager_name()`, `parse_image_reference()`.
2. Have all call sites import from `util` instead of maintaining local copies.
3. Consolidate the different glob implementations into one with clearly documented semantics (trailing `*`, `**` with path separator awareness, etc.).

## References
- Rust Design Patterns: Modularity, Single Responsibility
- Effective Rust Item 22: Minimize visibility

## Acceptance Criteria

- [x] Single `glob_match` implementation used by `automerge`, `grouping`, `replacement`, and `manager::regex`
- [x] Single `manager_name` function shared by `grouping` and `dashboard`
- [x] Single `parse_image_reference` function shared by `manager::docker` and `manager::helm`
- [x] `cargo test` passes — all existing tests still work against the shared implementations
