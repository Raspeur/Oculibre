#!/usr/bin/env bash
# Turns on the repository's protections (#27): rulesets (.github/rulesets), Dependabot, and the two
# that need a public repository on a free plan (secret scanning, private vulnerability reporting).
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
