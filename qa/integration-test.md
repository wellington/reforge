# Integration Test: Reforge E2E

## Overview

End-to-end integration test that builds reforge and runs it against the real `poc/configurations` repo on `gitlab.mgmt.procoregov-qa.internal`. Validates that reforge can scan a repository, detect outdated dependencies, and open merge requests to update them.

## Prerequisites

| Requirement | Source | How to verify |
|---|---|---|
| GitLab access (read + MR create/update) | `~/.git-credentials` (oauth2 token for `gitlab.mgmt.procoregov-qa.internal`) | `make verify-access` |
| Artifactory access (read-only) | `ARTIFACTORY_API_KEY` env var | `make verify-access` |
| Rust toolchain | `rustup` / system install | `cargo --version` |

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

1. **Verify access** — Confirms GitLab API token works (can read project metadata and create MRs) and Artifactory API key is valid (can reach the OCI registry).
2. **Teardown** — Finds all open MRs in `poc/configurations` with the `reforge-e2e-test` label, closes them, and deletes their source branches. Ensures a clean slate.
3. **Build reforge** — Runs `cargo build --release`.
4. **Run reforge** — Executes reforge with `qa/reforge-e2e.toml` against `poc/configurations`. This config mirrors production but adds the `reforge-e2e-test` label to all created MRs.
5. **Validate** — Queries GitLab for MRs with the `reforge-e2e-test` label. Verifies at least one MR was created. Reports MR URLs and pipeline status.

## Success Criteria

- [ ] `make verify-access` passes (both GitLab and Artifactory accessible)
- [ ] `make teardown` closes all previous test MRs without errors
- [ ] Reforge exits successfully (exit code 0)
- [ ] At least one MR is created with the `reforge-e2e-test` label
- [ ] MR titles follow the expected format (`Update <dep> to <version>`)
- [ ] CI pipelines on MR branches pass (or no pipeline is configured)

## Manual Verification Steps

These steps require human judgment and cannot be fully automated:

1. **MR content review** — Open at least one created MR and verify the diff makes sense (correct file modified, version bumped correctly, no unrelated changes).
2. **Registry version accuracy** — Spot-check that the "new version" in the MR title actually exists in the source registry (Docker Hub, Artifactory).
3. **Dashboard issue** — If dashboard is enabled, check that the Dependency Dashboard issue in `poc/configurations` was created/updated.

## Known Issues

- The GitLab instance uses a self-signed TLS certificate, so all API calls require `-k` (curl) or `insecure = true` (reforge config).
- If all dependencies are already up to date, reforge will create zero MRs. This is correct behavior but will cause the validation step to fail. In this case, manually verify the dashboard shows "all up to date" and consider the test passed.
- Pipeline status checking has a 5-minute timeout. Long-running pipelines may not complete within that window.
- **Pre-existing MRs without `reforge-e2e-test` label**: If production reforge MRs already exist for the same updates, reforge will skip creation (branch already exists). The teardown only removes MRs with the e2e label. To get a clean run, manually close all open reforge MRs and delete their branches first, or merge them.
- No CI pipeline is currently configured in `poc/configurations`, so pipeline validation reports "none" for all MRs. This is expected until `.gitlab-ci.yml` is added.

## Teardown Details

- MRs are identified by the `reforge-e2e-test` label in the `poc/configurations` project (ID 4).
- The teardown script closes MRs and deletes their source branches.
- If a branch was already deleted (e.g., manually), the branch deletion step will log a warning but not fail.
- The dashboard issue (if any) is NOT torn down — it gets updated in place by subsequent runs.

## Configuration

The e2e test uses `qa/reforge-e2e.toml`, which is a copy of the production config with these differences:
- `merge_request.labels` includes `reforge-e2e-test` for test identification
- All other settings match production to ensure realistic testing
