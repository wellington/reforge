# Grouped / Monorepo Updates

Combine multiple dependency updates into a single MR based on configurable grouping rules.

## Why
- Reduces MR noise for low-risk batch updates (e.g., all patch bumps in one MR)
- Monorepos with many services benefit from coordinated updates
- Some teams prefer reviewing related changes together

## What's needed
- **Grouping rules config**: Group by update type (all patches together), by manager (all docker updates), by custom pattern, or by service/path
- **Multi-file commits**: Combine multiple `FileUpdate` results into a single branch and commit
- **MR body aggregation**: Table listing all updates included in the grouped MR
- **Conflict handling**: If one update in a group fails, create the MR with the rest and note the failure

## Estimated scope
~300-400 lines. Grouping logic in orchestrator, multi-update commit assembly, expanded MR body template.
