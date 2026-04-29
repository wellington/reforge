#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG="$REPO_ROOT/qa/reforge-e2e.toml"

if [[ -z "${REFORGE_TOKEN:-}" ]]; then
    echo "FAIL: REFORGE_TOKEN is not set"
    exit 1
fi

if [[ -z "${REFORGE_GITLAB_URL:-}" ]]; then
    echo "FAIL: REFORGE_GITLAB_URL is not set"
    exit 1
fi

echo "=== Building reforge ==="
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"

echo "=== Running reforge against test project ==="
"$REPO_ROOT/target/release/reforge" --config "$CONFIG" --log-level info
echo "=== Reforge completed successfully ==="
