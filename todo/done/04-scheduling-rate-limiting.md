# Scheduling and Rate Limiting MR Creation

Limit the number of concurrent open MRs and control when new ones are created.

## Why
- A first run against a stale repo can create dozens of MRs and overwhelm the team
- Teams need to control update velocity to match their review capacity
- Prevents CI resource exhaustion from too many parallel update pipelines

## What's needed
- **Max open MRs**: Config option (e.g., `max_open_mrs = 5`) to cap how many reforge MRs can be open simultaneously
- **Priority ordering**: When at the limit, prioritize security updates, then major, then minor, then patch
- **Schedule window**: Optional cron-like config for when MRs can be created (e.g., only weekdays, only outside business hours)
- **Backoff on existing MRs**: Count existing open MRs before creating new ones, skip if at limit

## Estimated scope
~150-200 lines. Config additions, counting logic in orchestrator before MR creation loop, optional cron parsing.
