# Project Naming: Reforge

**Last updated:** 2026-04-04
**Type:** Decision
**Status:** Active

## Description

The project is named **reforge**. All user-facing APIs, CLI names, env vars, config files, branch prefixes, labels, and error types use the `reforge` name. The project also accepts `RENOVATE_` prefixed env vars as a migration path from Renovate.

## Context

Applies to all naming decisions: binary name, config file, env vars, MR labels, branch prefixes, error types, MR body text.

## Details

### Naming Convention

| Thing | Name |
|-------|------|
| Binary | `reforge` |
| Cargo package | `reforge` |
| Config file | `reforge.toml` |
| Env var: token | `REFORGE_TOKEN` |
| Env var: GitLab URL | `REFORGE_GITLAB_URL` |
| Branch prefix | `reforge/` |
| MR labels | `reforge`, `automated` |
| Error type | `ReforgeError` |
| MR footer | `*This MR was automatically created by reforge.*` |

### Renovate Compatibility (Migration Support)

To ease transition from Renovate to Reforge, the following `RENOVATE_` env vars are accepted as fallbacks (REFORGE_ always takes precedence):

- `RENOVATE_TOKEN` → fallback for `REFORGE_TOKEN`
- `RENOVATE_GITLAB_URL` → fallback for `REFORGE_GITLAB_URL`

Note: These are `RENOVATE_` (not `RENOVATE_RS_`). The `_RS` suffix was an early internal name and has been fully removed.

### What NOT to Name "Renovate"

- Never use `renovate-rs` or `RENOVATE_RS` anywhere in new code
- The `renovate` name only appears in fallback env var lookups and in `implementation-plan.md` (historical reference)

## Summary

Use `reforge` for all new naming. Accept `RENOVATE_` env vars for migration compatibility. Never use `RENOVATE_RS_`.

## Notes
- The `implementation-plan.md` file still references the old `renovate-rs` naming — it's a historical document and was intentionally not updated.
- **See also:** [project-architecture.md](project-architecture.md)
