#!/usr/bin/env bash
set -euo pipefail

GITLAB_URL="https://gitlab.mgmt.procoregov-qa.internal"
GITLAB_PROJECT="poc%2Fconfigurations"
E2E_LABEL="reforge-e2e-test"

if [[ -n "${REFORGE_TOKEN:-}" ]]; then
    TOKEN="$REFORGE_TOKEN"
elif [[ -f "$HOME/.git-credentials" ]]; then
    TOKEN=$(grep "gitlab.mgmt.procoregov-qa.internal" "$HOME/.git-credentials" \
        | head -1 \
        | sed 's|.*://[^:]*:\([^@]*\)@.*|\1|')
fi

if [[ -z "${TOKEN:-}" ]]; then
    echo "FAIL: No GitLab token. Set REFORGE_TOKEN or configure ~/.git-credentials"
    exit 1
fi

echo "=== Tearing down previous e2e test MRs ==="

MRS=$(curl -sk \
    -H "PRIVATE-TOKEN: $TOKEN" \
    "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT/merge_requests?state=opened&labels=$E2E_LABEL&per_page=100")

MR_COUNT=$(echo "$MRS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null || echo "0")

if [[ "$MR_COUNT" == "0" ]]; then
    echo "No open MRs with label '$E2E_LABEL' found. Nothing to tear down."
    exit 0
fi

echo "Found $MR_COUNT open MR(s) with label '$E2E_LABEL'"

echo "$MRS" | python3 -c "
import sys, json
for mr in json.load(sys.stdin):
    print(f\"{mr['iid']}|{mr['source_branch']}|{mr['title']}\")
" | while IFS='|' read -r IID BRANCH TITLE; do
    echo -n "  Closing MR !$IID ($TITLE)... "
    curl -sk -o /dev/null -w "" \
        -X PUT \
        -H "PRIVATE-TOKEN: $TOKEN" \
        -H "Content-Type: application/json" \
        -d '{"state_event":"close"}' \
        "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT/merge_requests/$IID"
    echo "closed"

    ENCODED_BRANCH=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$BRANCH', safe=''))")
    echo -n "  Deleting branch $BRANCH... "
    HTTP_CODE=$(curl -sk -o /dev/null -w "%{http_code}" \
        -X DELETE \
        -H "PRIVATE-TOKEN: $TOKEN" \
        "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT/repository/branches/$ENCODED_BRANCH")

    if [[ "$HTTP_CODE" == "204" || "$HTTP_CODE" == "200" ]]; then
        echo "deleted"
    elif [[ "$HTTP_CODE" == "404" ]]; then
        echo "already gone (404)"
    else
        echo "warning: unexpected HTTP $HTTP_CODE"
    fi
done

echo "=== Teardown complete ==="
