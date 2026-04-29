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

- One background `agent` invocation per todo item, run sequentially (background agents cannot safely be parallelized due to branch conflicts and print-mode limitations).
- Each agent works on a feature branch created via `git worktree` to avoid conflicts.
- After each agent completes, the orchestrator verifies the work (`cargo check`, `cargo test`, file inspection) before merging.

### Agent Prompt Template

Each dispatched agent receives:
1. The full todo spec from `todo/NN-*.md`
2. Relevant source files to read and modify
3. A configurations repo path for integration testing (if available)
4. Registry credentials via env vars for integration tests (e.g., `REGISTRY_API_KEY`)
5. Explicit instructions to: create a feature branch, implement the feature, write tests, run `cargo check` + `cargo test`, commit with a descriptive message, and NOT push

### Integration Testing

- Use a local configurations repo with real YAML files for parser testing.
- Reforge can operate against the local filesystem (local git mode) rather than the GitLab API for development.

## Summary

Spawn one agent per todo, sequentially, using git worktrees for isolation. Verify each agent's output before proceeding. Use a local configurations repo and registry credentials for integration testing.

## Notes
- **See also:** [project-architecture.md](project-architecture.md)
- Background agent print mode does NOT work — agents must run in foreground or sequential background.
