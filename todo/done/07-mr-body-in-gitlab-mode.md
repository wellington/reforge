# Verify MR Body Content in GitLab Mode

## Priority: Low
## Status: Open

## Description

When creating MRs via the GitLab API, verify that the MR description body includes the expected markdown table with package/manager/file/current/new columns. The `build_group_mr_content()` method generates this, but it hasn't been verified against the live MRs.

Also verify that when `changelog`, `vulnerability`, and `lockfile` features are enabled, their output is correctly included in the MR body.

## Acceptance Criteria

- [ ] MR body contains the dependency update table
- [ ] Changelog section appears when enabled and available
- [ ] Vulnerability section appears when CVEs are found
- [ ] Chart.lock update commits appear alongside Chart.yaml updates
