# GitLab CI Pipeline for Reforge

## Priority: Medium
## Status: Open

## Description

Implement a `.gitlab-ci.yml` pipeline that builds and runs reforge as a scheduled pipeline on GitLab CI. GitLab runners (not yet provisioned) will execute reforge on a schedule to scan target projects for dependency updates.

## Details

- Build stage: compile the reforge binary (or use the existing Docker image)
- Scan stage: run `reforge --config reforge.toml` against configured projects
- Trigger: scheduled pipeline (GitLab CI schedules, not K8s CronJob)
- Secrets: `REFORGE_TOKEN` and `ARTIFACTORY_API_KEY` provided as CI/CD variables
- Config: `reforge.toml` checked into the repo or mounted via CI/CD file variables

## Blocked By

- GitLab runners not yet available

## Acceptance Criteria

- [ ] `.gitlab-ci.yml` with build and scan stages
- [ ] Scheduled pipeline trigger (e.g. every 6 hours)
- [ ] `REFORGE_TOKEN` and `ARTIFACTORY_API_KEY` read from CI/CD variables
- [ ] `reforge.toml` configuration for target projects
- [ ] Pipeline succeeds in dry-run mode on first run
