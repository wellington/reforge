# [HIGH] Replace stringly-typed fields with enums

## Priority: High
## Status: Open

## Description

Several configuration and API fields use `String` where a finite set of valid values exists. This allows invalid states at runtime that should be caught at compile time.

## Locations

### 1. `CommitAction.action` — `src/platform/gitlab.rs:28-31`
Only valid values are `"create"`, `"update"`, `"delete"`, `"move"`. Currently a `String`.

### 2. `VersioningConfig.pin_strategy` — `src/config.rs:200-211`
Only valid values are `"semver-patch"`, `"semver-minor"`, `"semver-major"`. Currently a `String` deserialized from TOML, then parsed via `PinStrategy::from_str()`.

### 3. `ManagersConfig.enabled` — `src/config.rs:181-193`
Only valid values are `"helm"`, `"docker"`. Currently `Vec<String>`.

### 4. `MergeRequestConfig.grouping` — `src/config.rs:320-327`
Only valid values are `"per-dependency"`, `"grouped"`. Currently a `String`.

### 5. `RegexManagerConfig.datasource` — `src/config.rs:106`
Only valid values are `"docker"`, `"helm-oci"`, `"helm-repo"`. Currently a `String` validated at runtime.

## Category
Type System / Anti-Patterns (Audit Checklist §3, §10)

## Issue
Stringly-typed fields require runtime validation, produce unhelpful error messages on typos, and force match arms to use catch-all `_` patterns that silently accept new invalid values. Using enums moves validation to deserialization time and makes exhaustive matching enforced by the compiler.

## Suggested fix direction
1. Define enums for each: `CommitActionKind`, `PinStrategy` (already exists — use it directly in config), `ManagerKind`, `GroupingMode`, `Datasource`.
2. Use `#[serde(rename_all = "kebab-case")]` for TOML/YAML compatibility.
3. Replace `PinStrategy::from_str()` inherent method with a proper `std::str::FromStr` implementation or use the enum directly in the config struct.

## References
- Effective Rust Item 1: Use the type system to express your data structures
- Rust Design Patterns Anti-Pattern: Stringly-typed API
- Rust API Guidelines C-CUSTOM-TYPE

## Acceptance Criteria

- [ ] At least `CommitAction.action`, `VersioningConfig.pin_strategy`, and `RegexManagerConfig.datasource` use enums
- [ ] `PinStrategy` implements `FromStr` or is used directly in `VersioningConfig` via serde
- [ ] Runtime validation code (e.g., `match datasource.as_str()` in `RegexManagerConfig::validate`) is removed in favor of serde deserialization errors
- [ ] `cargo check` and `cargo test` pass
