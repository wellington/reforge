# Dashboard File Not Committed in Local Mode

## Priority: Medium
## Status: Open

## Description

In local git mode, the dependency dashboard is written to disk as an untracked file but never committed. When the orchestrator switches branches during update processing, the dashboard file can be lost. It should either be committed to the default branch or written outside the git working tree.

## Current Behavior

1. Orchestrator creates update branches (checking out each one)
2. At the end, `dashboard::write_local_dashboard()` writes `DEPENDENCY_DASHBOARD.md`
3. The file is written to whatever branch is currently checked out (the last update branch)
4. The file is not `git add`'d or committed
5. If the user checks out main, the file is gone

## Expected Behavior

The dashboard should be committed to the default branch (main) after all update branches are created, or written to a path outside the repo.

## Acceptance Criteria

- [ ] In local mode, checkout main after creating update branches
- [ ] Commit `DEPENDENCY_DASHBOARD.md` to main
- [ ] Or: write dashboard to a configurable path outside the repo
