# Multi-Project Scanning

## Priority: Low
## Status: Open

## Description

The `scan.projects` config accepts a list of GitLab project paths, and the orchestrator loops over them. However, this has only been tested with a single project. Verify that scanning multiple projects in one run works correctly (rate limits are per-project, dashboard issues are per-project, etc.).

## Acceptance Criteria

- [ ] Scanning 2+ projects in one run creates correct MRs for each
- [ ] Rate limiting (`max_open_mrs`) applies per-project, not globally
- [ ] Dashboard issues are created per-project
- [ ] Errors in one project don't block processing of others
