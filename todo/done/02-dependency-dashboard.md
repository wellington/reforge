# Dependency Dashboard

Create and maintain a single GitLab issue per project that summarizes all pending dependency updates.

## Why
- Primary interface for teams to see update status at a glance
- Reduces notification noise compared to individual MR emails
- Lets teams selectively trigger or suppress updates

## What's needed
- **Dashboard issue creation**: Create a well-known issue (by title convention) if it doesn't exist
- **Issue body generation**: Grouped table of all dependencies — up-to-date, pending update, open MR, ignored
- **Idempotent updates**: On each run, update the issue body in place rather than creating new issues
- **Checkboxes for control**: Renovate uses checkboxes to let users trigger or suppress specific updates from the dashboard
- **Link to open MRs**: Each pending update row should link to its MR if one exists

## Estimated scope
~300-400 lines. New GitLab API methods for issue create/update/search. Dashboard rendering logic in orchestrator.
