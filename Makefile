.PHONY: e2e verify-access teardown run-reforge validate build \
        check fmt fmt-fix lint lint-fix pedantic test ci hooks

# ---------------------------------------------------------------------------
# Quality gate
#
# `check` is the fast, read-only gate: it never modifies source and fails on
# the first problem. This is what the pre-commit hook runs. Tests are kept
# separate (`test`) so the commit gate stays quick.
# ---------------------------------------------------------------------------

check: fmt lint

# Read-only: fails if anything is misformatted, prints the diff it wants.
fmt:
	cargo fmt --all -- --check

# Rewrites source in place.
fmt-fix:
	cargo fmt --all

# Read-only. --all-targets covers tests and benches, not just the binary.
# -D warnings turns findings into a non-zero exit so this can gate a commit.
lint:
	cargo clippy --all-targets --all-features -- -D warnings

# Rewrites source in place. Requires a clean tree unless --allow-dirty.
lint-fix:
	cargo clippy --fix --all-targets --all-features

# Advisory only, never gates: ~91 findings today. Chip away at these over time.
pedantic:
	cargo clippy --all-targets -- -W clippy::pedantic

test:
	cargo test --all-features

# Everything, for CI or a pre-push check.
ci: check test

# Point git at the tracked hooks directory (one-time, per clone).
hooks:
	git config core.hooksPath .githooks
	@echo "pre-commit hook active — bypass a single commit with 'git commit --no-verify'"

# ---------------------------------------------------------------------------
# End-to-end QA
# ---------------------------------------------------------------------------

e2e: verify-access teardown run-reforge validate

verify-access:
	@qa/scripts/verify-access.sh

teardown:
	@qa/scripts/teardown.sh

build:
	cargo build --release

run-reforge: build
	@qa/scripts/run-reforge.sh

validate:
	@qa/scripts/validate.sh
