#!/usr/bin/env bash
set -euo pipefail

echo "=== Verifying access to required services ==="

# --- Required environment variables ---
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
GITLAB_TOKEN="$REFORGE_TOKEN"
GITLAB_PROJECT=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$REFORGE_GITLAB_PROJECT', safe=''))")

echo -n "GitLab API (read)... "
HTTP_CODE=$(curl -sk -o /dev/null -w "%{http_code}" \
    -H "PRIVATE-TOKEN: $GITLAB_TOKEN" \
    "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT")

if [[ "$HTTP_CODE" == "200" ]]; then
    echo "OK ($HTTP_CODE)"
else
    echo "FAIL (HTTP $HTTP_CODE)"
    exit 1
fi

echo -n "GitLab API (MR write)... "
HTTP_CODE=$(curl -sk -o /dev/null -w "%{http_code}" \
    -H "PRIVATE-TOKEN: $GITLAB_TOKEN" \
    "$GITLAB_URL/api/v4/projects/$GITLAB_PROJECT/merge_requests?state=opened&per_page=1")

if [[ "$HTTP_CODE" == "200" ]]; then
    echo "OK ($HTTP_CODE)"
else
    echo "FAIL (HTTP $HTTP_CODE)"
    exit 1
fi

# --- Optional: OCI registry check ---
if [[ -n "${REGISTRY_API_KEY:-}" && -n "${REGISTRY_URL:-}" ]]; then
    echo -n "OCI registry ($REGISTRY_URL)... "
    HTTP_CODE=$(curl -sk -o /dev/null -w "%{http_code}" \
        -H "Authorization: Bearer $REGISTRY_API_KEY" \
        "$REGISTRY_URL/v2/")

    if [[ "$HTTP_CODE" == "200" ]]; then
        echo "OK ($HTTP_CODE)"
    elif [[ "$HTTP_CODE" == "401" ]]; then
        echo "FAIL: Authentication rejected (HTTP 401). Check REGISTRY_API_KEY."
        exit 1
    else
        echo "FAIL (HTTP $HTTP_CODE)"
        exit 1
    fi
else
    echo "Skipping OCI registry check (REGISTRY_API_KEY or REGISTRY_URL not set)"
fi

echo "=== All access checks passed ==="
