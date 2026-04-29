# Integration Test: Reforge E2E

## Overview

End-to-end integration test that builds reforge and runs it against a real GitLab instance. Validates that reforge can scan a repository, detect outdated dependencies, and open merge requests to update them.

## Prerequisites

| Requirement | Source | How to verify |
|---|---|---|
| GitLab access (read + MR create/update) | `REFORGE_TOKEN` env var | `make verify-access` |
| GitLab URL | `REFORGE_GITLAB_URL` env var | `make verify-access` |
| GitLab project | `REFORGE_GITLAB_PROJECT` env var | `make verify-access` |
| Registry access (read-only, optional) | `REGISTRY_API_KEY` + `REGISTRY_URL` env vars | `make verify-access` |
| Rust toolchain | `rustup` / system install | `cargo --version` |

## Environment Variables

```bash
export REFORGE_TOKEN="glpat-xxxxxxxxxxxxxxxxxxxx"
export REFORGE_GITLAB_URL="https://gitlab.example.com"
export REFORGE_GITLAB_PROJECT="my-group/my-project"
# Optional: for private OCI registries
export REGISTRY_API_KEY="your-api-key"
export REGISTRY_URL="https://registry.example.com"
```

## Running

```bash
make e2e
```

Individual steps can be run separately for debugging:

```bash
make verify-access    # Check credentials and connectivity
make teardown         # Close previous test MRs
make run-reforge      # Build and run reforge
make validate         # Check MRs were created
```

## What It Does

1. **Verify access** — Confirms GitLab API token works (can read project metadata and create MRs) and optional registry access is valid.
2. **Teardown** — Finds all open MRs with the `reforge-e2e-test` label, closes them, and deletes their source branches. Ensures a clean slate.
3. **Build reforge** — Runs `cargo build --release`.
4. **Run reforge** — Executes reforge with `qa/reforge-e2e.toml` against the configured project. This config adds the `reforge-e2e-test` label to all created MRs.
5. **Validate** — Queries GitLab for MRs with the `reforge-e2e-test` label. Verifies at least one MR was created. Reports MR URLs and pipeline status.

## Success Criteria

- [ ] `make verify-access` passes (GitLab accessible, optional registry accessible)
- [ ] `make teardown` closes all previous test MRs without errors
- [ ] Reforge exits successfully (exit code 0)
- [ ] At least one MR is created with the `reforge-e2e-test` label
- [ ] MR titles follow the expected format (`Update <dep> to <version>`)
- [ ] CI pipelines on MR branches pass (or no pipeline is configured)

## Manual Verification Steps

These steps require human judgment and cannot be fully automated:

1. **MR content review** — Open at least one created MR and verify the diff makes sense (correct file modified, version bumped correctly, no unrelated changes).
2. **Registry version accuracy** — Spot-check that the "new version" in the MR title actually exists in the source registry.
3. **Dashboard issue** — If dashboard is enabled, check that the Dependency Dashboard issue was created/updated.

## Known Issues

- If the GitLab instance uses a self-signed TLS certificate, all API calls require `-k` (curl) or `insecure = true` (reforge config).
- If all dependencies are already up to date, reforge will create zero MRs. This is correct behavior but will cause the validation step to fail. In this case, manually verify the dashboard shows "all up to date" and consider the test passed.
- Pipeline status checking has a 5-minute timeout. Long-running pipelines may not complete within that window.
- **Pre-existing MRs without `reforge-e2e-test` label**: If production reforge MRs already exist for the same updates, reforge will skip creation (branch already exists). The teardown only removes MRs with the e2e label.

## Teardown Details

- MRs are identified by the `reforge-e2e-test` label.
- The teardown script closes MRs and deletes their source branches.
- If a branch was already deleted (e.g., manually), the branch deletion step will log a warning but not fail.
- The dashboard issue (if any) is NOT torn down — it gets updated in place by subsequent runs.

## Configuration

The e2e test uses `qa/reforge-e2e.toml`, which should be customized for your environment:
- Update `[gitlab].url` or rely on `REFORGE_GITLAB_URL` env var
- Update `[scan].projects` to match `REFORGE_GITLAB_PROJECT`
- `merge_request.labels` includes `reforge-e2e-test` for test identification
