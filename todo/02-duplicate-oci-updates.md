# Duplicate OCI Chart Updates

## Priority: Medium
## Status: Open

## Description

When both the Helm manager (via `Chart.yaml`) and a regex manager (via `apps/*/login.yaml`) detect the same OCI chart dependency, reforge creates two identical update candidates. In the current `poc/configurations` run, `stateless-http-service 14.1.0 -> 14.9.0` appeared twice in the dry-run report and caused a "Branch already exists" error during MR creation.

## Reproduction

```
INFO Update available: stateless-http-service 14.1.0 -> 14.9.0   # from Chart.yaml
INFO Update available: stateless-http-service 14.1.0 -> 14.9.0   # from apps/app/login.yaml
...
ERROR Failed to create MR for group 'helm-stateless-http-service': GitLab API error: 400 {"message":"Branch already exists"}
```

## Expected Behavior

Reforge should deduplicate update candidates when the same dependency+version pair is detected from multiple files. Ideally, a single MR should contain updates to all files where the dependency appears.

## Acceptance Criteria

- [ ] Deduplicate candidates by (dependency name, registry source, new version) before creating branches
- [ ] When the same dep is found in multiple files, the single MR updates all of them
- [ ] No "Branch already exists" errors from duplicate detection
