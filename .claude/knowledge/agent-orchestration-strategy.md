# Agent Orchestration Strategy

**Last updated:** 2026-04-04
**Type:** Decision
**Status:** Active

## Description

Todo items (features in `todo/`) are implemented by spawning separate background Cursor agent CLI instances — one per todo item — to minimize context overflow and keep token costs low. The orchestrating agent (this session) dispatches and monitors them.

## Context

Applies when working through the todo backlog. Each agent gets a self-contained prompt with full context about the codebase, the specific feature to implement, and testing instructions.

## Details

### Execution Model

- One background `agent` invocation per todo item, run sequentially (background agents cannot safely be parallelized due to branch conflicts and print-mode limitations — see `orchestrating-cursor-agents.mdc`).
- Each agent works on a feature branch created via `git worktree` to avoid conflicts.
- After each agent completes, the orchestrator verifies the work (`cargo check`, `cargo test`, file inspection) before merging.

### Agent Prompt Template

Each dispatched agent receives:
1. The full todo spec from `todo/NN-*.md`
2. Relevant source files to read and modify
3. The configurations repo path for integration testing
4. The `ARTIFACTORY_API_KEY` env var for registry integration tests against `oci-charts.artifacts.procoretech.com`
5. Explicit instructions to: create a feature branch, implement the feature, write tests, run `cargo check` + `cargo test`, commit with a descriptive message, and NOT push

### Integration Testing

- The configurations repo at `/home/drew/src/gitlab.mgmt.procoregov-qa.internal/poc/configurations` provides real YAML files for parser testing.
- No Docker registry is available locally, so there is no container build/push step.
- Reforge operates against the local filesystem (local git mode, todo/01) rather than the GitLab API for this development cycle.

### No GitLab API Available

Since there's no reachable GitLab instance for API testing, the first priority is implementing local git support (todo/01) so that reforge can operate against the local configurations repo checkout.

## Summary

Spawn one agent per todo, sequentially, using git worktrees for isolation. Verify each agent's output before proceeding. Use the local configurations repo and Artifactory API key for integration testing.

## Notes
- **See also:** [project-architecture.md](project-architecture.md)
- Background agent print mode does NOT work — agents must run in foreground or sequential background.
- The `ARTIFACTORY_API_KEY` env var is available in the shell environment.
