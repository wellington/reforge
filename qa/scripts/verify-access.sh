#!/usr/bin/env bash
set -euo pipefail

GITLAB_URL="https://gitlab.mgmt.procoregov-qa.internal"
GITLAB_PROJECT="poc%2Fconfigurations"

echo "=== Verifying access to required services ==="

# --- GitLab token ---
if [[ -n "${REFORGE_TOKEN:-}" ]]; then
    GITLAB_TOKEN="$REFORGE_TOKEN"
elif [[ -f "$HOME/.git-credentials" ]]; then
    GITLAB_TOKEN=$(grep "gitlab.mgmt.procoregov-qa.internal" "$HOME/.git-credentials" \
        | head -1 \
        | sed 's|.*://[^:]*:\([^@]*\)@.*|\1|')
fi

if [[ -z "${GITLAB_TOKEN:-}" ]]; then
    echo "FAIL: No GitLab token found. Set REFORGE_TOKEN or add credentials to ~/.git-credentials"
    exit 1
fi
export REFORGE_TOKEN="$GITLAB_TOKEN"

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

# --- Artifactory ---
if [[ -z "${ARTIFACTORY_API_KEY:-}" ]]; then
    echo "FAIL: ARTIFACTORY_API_KEY is not set"
    exit 1
fi

echo -n "Artifactory OCI registry... "
HTTP_CODE=$(curl -sk -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer $ARTIFACTORY_API_KEY" \
    "https://oci-charts.artifacts.procoretech.com/v2/")

if [[ "$HTTP_CODE" == "200" ]]; then
    echo "OK ($HTTP_CODE)"
elif [[ "$HTTP_CODE" == "401" ]]; then
    echo "FAIL: Authentication rejected (HTTP 401). Check ARTIFACTORY_API_KEY."
    exit 1
else
    echo "FAIL (HTTP $HTTP_CODE)"
    exit 1
fi

echo "=== All access checks passed ==="
