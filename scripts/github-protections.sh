#!/usr/bin/env bash
# Turns on the repository's protections (#27): rulesets (.github/rulesets), Dependabot, merged
# branches deleted, the release environment, and what needs a public repository on a free plan
# (secret scanning, private vulnerability reporting, approval of releases).
# Run it now, then again right after making the repository public:
#   gh repo edit Raspeur/Oculibre --visibility public --accept-visibility-change-consequences
#   bash scripts/github-protections.sh
# Safe to run again: what is already on stays, existing rulesets (by name) are left alone.
set -euo pipefail
REPO=${REPO:-Raspeur/Oculibre}
ROOT=$(cd "$(dirname "$0")/.." && pwd)

visibility=$(gh api "repos/$REPO" --jq .visibility)
echo "$REPO is $visibility"

# Some protections exist only for public repositories on a free plan: try, and say so if refused.
try() {
    local what=$1
    shift
    if "$@" >/dev/null 2>&1; then
        echo "== $what: on"
    else
        echo "== $what: refused (needs a public repository?)"
    fi
}

try "Private vulnerability reporting" gh api -X PUT "repos/$REPO/private-vulnerability-reporting"
try "Secret scanning and push protection" gh api -X PATCH "repos/$REPO" \
    -F 'security_and_analysis[secret_scanning][status]=enabled' \
    -F 'security_and_analysis[secret_scanning_push_protection][status]=enabled'
try "Dependabot alerts" gh api -X PUT "repos/$REPO/vulnerability-alerts"
try "Dependabot security updates" gh api -X PUT "repos/$REPO/automated-security-fixes"

# A pull request's branch is deleted once it is merged (it can be restored from the pull request).
try "Delete merged branches" gh repo edit "$REPO" --delete-branch-on-merge

# The "release" environment holds the Android signing key (.github/RELEASING.md): only v* tags may
# use it, and, once the repository is public, each release waits for the owner's approval.
gh api -X PUT "repos/$REPO/environments/release" --input - >/dev/null <<'JSON'
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}
JSON
if ! gh api "repos/$REPO/environments/release/deployment-branch-policies" --jq '.branch_policies[].name' | grep -qxF 'v*'; then
    gh api -X POST "repos/$REPO/environments/release/deployment-branch-policies" -f name='v*' -f type=tag >/dev/null
fi
echo "== Release environment: v* tags only"
owner_id=$(gh api "users/${REPO%%/*}" --jq .id)
try "Release environment: owner's approval" gh api -X PUT "repos/$REPO/environments/release" --input - <<JSON
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true},
 "reviewers": [{"type": "User", "id": $owner_id}], "prevent_self_review": false}
JSON

echo "== Rulesets"
existing=$(gh api "repos/$REPO/rulesets" --jq '.[].name')
for file in "$ROOT"/.github/rulesets/*.json; do
    name=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['name'])" "$file")
    if grep -qxF "$name" <<<"$existing"; then
        echo "   $name: already there"
    else
        gh api -X POST "repos/$REPO/rulesets" --input "$file" >/dev/null
        echo "   $name: created"
    fi
done

echo "== Done. Check Settings › Rules, Settings › Code security."
