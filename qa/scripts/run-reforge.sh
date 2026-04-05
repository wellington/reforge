#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG="$REPO_ROOT/qa/reforge-e2e.toml"

if [[ -n "${REFORGE_TOKEN:-}" ]]; then
    export REFORGE_TOKEN
elif [[ -f "$HOME/.git-credentials" ]]; then
    REFORGE_TOKEN=$(grep "gitlab.mgmt.procoregov-qa.internal" "$HOME/.git-credentials" \
        | head -1 \
        | sed 's|.*://[^:]*:\([^@]*\)@.*|\1|')
    export REFORGE_TOKEN
fi

if [[ -z "${REFORGE_TOKEN:-}" ]]; then
    echo "FAIL: No GitLab token. Set REFORGE_TOKEN or configure ~/.git-credentials"
    exit 1
fi

echo "=== Building reforge ==="
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"

echo "=== Running reforge against poc/configurations ==="
"$REPO_ROOT/target/release/reforge" --config "$CONFIG" --log-level info
echo "=== Reforge completed successfully ==="
