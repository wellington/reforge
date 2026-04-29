#!/usr/bin/env bash
set -euo pipefail

E2E_LABEL="reforge-e2e-test"
PIPELINE_TIMEOUT=300  # seconds

if [[ -z "${REFORGE_TOKEN:-}" ]]; then
    echo "FAIL: REFORGE_TOKEN is not set"
    exit 1
fi

if [[ -z "${REFORGE_GITLAB_URL:-}" ]]; then
    echo "FAIL: REFORGE_GITLAB_URL is not set"
    exit 1
fi

if [[ -z "${REFORGE_GITLAB_PROJECT:-}" ]]; then
    echo "FAIL: REFORGE_GITLAB_PROJECT is not set (e.g., 'my-group/my-project')"
    exit 1
fi

GITLAB_URL="$REFORGE_GITLAB_URL"
TOKEN="$REFORGE_TOKEN"
GITLAB_PROJECT=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$REFORGE_GITLAB_PROJECT', safe=''))")

echo "=== Validating e2e test results ==="

MRS=$(curl -sk \
    -H "PRIVATE-TOKEN: $TOKEN" \
    "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT/merge_requests?state=opened&labels=$E2E_LABEL&per_page=100")

MR_COUNT=$(echo "$MRS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null || echo "0")

if [[ "$MR_COUNT" == "0" ]]; then
    echo "WARN: No MRs found with label '$E2E_LABEL'."
    echo "This may mean all dependencies are already up to date."
    echo "Check the Dependency Dashboard issue for confirmation."
    exit 1
fi

echo "Found $MR_COUNT MR(s) with label '$E2E_LABEL':"
echo ""

FAILED=0

echo "$MRS" | python3 -c "
import sys, json
for mr in json.load(sys.stdin):
    print(f\"{mr['iid']}|{mr['source_branch']}|{mr['title']}|{mr['web_url']}\")
" | while IFS='|' read -r IID BRANCH TITLE URL; do
    echo "  MR !$IID: $TITLE"
    echo "    URL: $URL"
    echo "    Branch: $BRANCH"

    # Check for pipeline on the MR's source branch
    PIPELINES=$(curl -sk \
        -H "PRIVATE-TOKEN: $TOKEN" \
        "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT/merge_requests/$IID/pipelines")

    PIPELINE_COUNT=$(echo "$PIPELINES" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null || echo "0")

    if [[ "$PIPELINE_COUNT" == "0" ]]; then
        echo "    Pipeline: none (no CI configured or not yet triggered)"
        continue
    fi

    PIPELINE_ID=$(echo "$PIPELINES" | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['id'])")
    PIPELINE_STATUS=$(echo "$PIPELINES" | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['status'])")

    if [[ "$PIPELINE_STATUS" == "success" ]]; then
        echo "    Pipeline #$PIPELINE_ID: $PIPELINE_STATUS"
        continue
    fi

    if [[ "$PIPELINE_STATUS" == "failed" || "$PIPELINE_STATUS" == "canceled" ]]; then
        echo "    Pipeline #$PIPELINE_ID: $PIPELINE_STATUS  << FAILURE"
        FAILED=1
        continue
    fi

    # Pipeline is running/pending — wait for it
    echo -n "    Pipeline #$PIPELINE_ID: $PIPELINE_STATUS (waiting"
    ELAPSED=0
    INTERVAL=15
    while [[ "$ELAPSED" -lt "$PIPELINE_TIMEOUT" ]]; do
        sleep "$INTERVAL"
        ELAPSED=$((ELAPSED + INTERVAL))
        PIPELINE_STATUS=$(curl -sk \
            -H "PRIVATE-TOKEN: $TOKEN" \
            "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT/pipelines/$PIPELINE_ID" \
            | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])")

        echo -n "."

        if [[ "$PIPELINE_STATUS" == "success" ]]; then
            echo " $PIPELINE_STATUS)"
            break
        elif [[ "$PIPELINE_STATUS" == "failed" || "$PIPELINE_STATUS" == "canceled" ]]; then
            echo " $PIPELINE_STATUS)"
            FAILED=1
            break
        fi
    done

    if [[ "$ELAPSED" -ge "$PIPELINE_TIMEOUT" ]]; then
        echo " timeout after ${PIPELINE_TIMEOUT}s, last status: $PIPELINE_STATUS)"
    fi
done

echo ""
if [[ "$FAILED" -gt 0 ]]; then
    echo "=== Validation FAILED: one or more pipelines failed ==="
    exit 1
fi

echo "=== Validation passed: $MR_COUNT MR(s) created ==="
