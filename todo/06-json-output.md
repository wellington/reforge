# JSON Output Mode

## Priority: Low
## Status: Open

## Description

The CLI has a `--json` flag defined but it's not wired up. Dry-run output is currently only a formatted text table. JSON output would enable piping results into other tools (jq, CI scripts, dashboards).

## Details

The `Cli` struct has:
```rust
#[arg(long)]
json: bool,
```

But `cli.json` is never read by the orchestrator.

## Acceptance Criteria

- [ ] `--dry-run --json` outputs a JSON array of update candidates
- [ ] Each entry includes: `name`, `manager`, `file_path`, `current_version`, `new_version`, `registry`
- [ ] Output is valid JSON parseable by `jq`
