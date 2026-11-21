# Repository rulesets

Applied by `scripts/github-protections.sh`.

- `main.json`: the default branch takes changes through pull requests only: no direct push, no
  force push, no deletion; one approving review from a code owner (`.github/CODEOWNERS`), stale
  approvals dismissed on new pushes, conversations resolved; the CI checks *Desktop (Linux)*,
  *Desktop (Windows)* and *Android* green. Repository admins may merge their own pull requests
  without a review (`bypass_mode: pull_request`), but cannot push to `main` directly either.
- `release-tags.json`: only admins create, move or delete `v*` tags (releases).

Change a ruleset here, then in *Settings › Rules › Rulesets* (or rerun the script after deleting it).
