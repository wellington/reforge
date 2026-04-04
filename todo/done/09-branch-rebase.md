# Branch Rebase / Conflict Resolution

Automatically rebase or recreate update branches when the target branch moves forward.

## Why
- Stale MRs with merge conflicts accumulate and require manual intervention
- CI results on outdated branches are unreliable
- Teams lose trust in the tool if MRs can't be merged cleanly

## What's needed
- **Conflict detection**: Check if existing update MRs have merge conflicts (GitLab MR API exposes this)
- **Rebase via API**: Use GitLab's rebase endpoint (`PUT /merge_requests/:iid/rebase`) when possible
- **Recreate strategy**: If rebase fails, delete the branch, recreate from latest default branch, and force-push the update commit
- **Preserve MR metadata**: Keep the same MR open (update source branch) rather than closing and reopening to preserve review comments
- **Configurable behavior**: Option to choose between rebase, recreate, or ignore stale MRs

## Estimated scope
~250-350 lines. New GitLab API methods for rebase and conflict detection, staleness check loop in orchestrator.
