# Local Git Support

Add an alternative operating mode that shells out to `git` for local repo management instead of operating entirely through the GitLab API.

## Why
- Enables offline operation and faster file access for large repos
- Unlocks compatibility with non-GitLab platforms without needing their API implementations
- Useful when running outside CI (e.g., developer workstations)

## What's needed
- **`git.rs` module**: Wrapper around `git` CLI commands (clone, checkout, branch, add, commit, push, status) using `tokio::process::Command`
- **Filesystem-based file reading**: Read managed files from local clone instead of GitLab Repository Files API
- **Dual-mode orchestrator**: Strategy pattern or enum to switch between API mode and local git mode
- **Temp directory management**: Clone repos into temp dirs, clean up after runs, handle interrupted runs
- **Auth forwarding**: Support SSH keys, credential helpers, and token-based HTTPS for `git push`
- **Git error handling**: Parse exit codes and stderr from git CLI into structured errors

## Estimated scope
~500-700 lines of new code, moderate refactor of `orchestrator.rs`, new integration tests requiring a real git binary.
