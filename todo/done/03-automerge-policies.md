# Automerge with Policy Controls

Extend automerge beyond a simple boolean to support per-dependency and per-update-type policies.

## Why
- Patch updates are low risk and should merge automatically after CI passes
- Minor/major updates need human review
- Different dependencies have different risk profiles (internal vs third-party)

## What's needed
- **Policy config**: Per-dependency or per-pattern automerge rules in `reforge.toml` (e.g., automerge patches for `nginx`, require approval for anything major)
- **Update type classification**: Tag each update as patch/minor/major based on semver diff
- **MR merge API**: Use GitLab's `merge_when_pipeline_succeeds` or `merge` endpoint based on policy
- **Minimum age**: Optional delay before automerging (e.g., wait 3 days for community to surface regressions)
- **Required status checks**: Only automerge if specific CI jobs pass

## Estimated scope
~200-300 lines. Config schema changes, policy evaluation logic, additional GitLab API calls for merge status.
