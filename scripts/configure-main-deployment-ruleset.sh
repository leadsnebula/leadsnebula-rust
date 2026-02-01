#!/bin/bash
# Create or update the "Main Deployment" ruleset for the main branch via GitHub CLI.
# Requires: gh auth login, and repo admin (or appropriate token with Administration: write).
#
# The ruleset:
#   - Applies only to refs/heads/main
#   - Requires pull requests (1 approval) before merge
#   - Disallows force push (non_fast_forward)
#   - To require Rust CI: in GitHub UI, Rules → Main Deployment → Add rule → Require status checks
#
# Usage:
#   ./scripts/configure-main-deployment-ruleset.sh
#   ./scripts/configure-main-deployment-ruleset.sh create   # create (default)
#   ./scripts/configure-main-deployment-ruleset.sh delete  # delete ruleset named "Main Deployment"
#   REPO=owner/repo ./scripts/configure-main-deployment-ruleset.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-}"
ACTION="${1:-create}"

if [ -z "$REPO" ]; then
  if git -C "$SCRIPT_DIR/.." rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    REMOTE=$(git -C "$SCRIPT_DIR/.." remote get-url origin 2>/dev/null || true)
    if [[ "$REMOTE" =~ github\.com[:/]([^/]+)/([^/]+?)(\.git)?$ ]]; then
      REPO="${BASH_REMATCH[1]}/${BASH_REMATCH[2]%.git}"
    fi
  fi
fi
if [ -z "$REPO" ]; then
  echo "Could not determine repo. Set REPO=owner/repo or run from a git repo with origin pointing to GitHub." >&2
  exit 1
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "Run: gh auth login" >&2
  exit 1
fi

echo "Using repo: ${REPO}"

if ! gh api "repos/${REPO}" --silent >/dev/null 2>&1; then
  echo "Could not access repo ${REPO} (404 = not found or no access)." >&2
  echo "Rulesets require a token with repo Administration (write)." >&2
  echo "  Classic PAT: scope 'admin:repo_hook' or full repo admin." >&2
  echo "  Fine-grained PAT: Administration = 'Read and write' for this repo." >&2
  exit 1
fi
REPO_ID=$(gh api "repos/${REPO}" -q .id)
if [ -z "$REPO_ID" ]; then
  echo "Could not get repository ID for ${REPO}" >&2
  exit 1
fi

# Rulesets API requires repo Administration (write); 404 often means missing scope
if ! gh api "repos/${REPO}/rulesets" --silent >/dev/null 2>&1; then
  echo "Could not access rulesets for ${REPO} (404 = no access or rulesets not available)." >&2
  echo "Rulesets require a token with repo Administration (write)." >&2
  echo "  Classic PAT: add scope 'admin:repo_hook' or use full repo admin." >&2
  echo "  Fine-grained PAT: set Administration to 'Read and write' for this repo." >&2
  echo "  Then run: gh auth refresh -s admin:repo_hook  # or re-login with new token" >&2
  exit 1
fi

RULESET_JSON="${SCRIPT_DIR}/main-deployment-ruleset.json"
if [ ! -f "$RULESET_JSON" ]; then
  echo "Ruleset JSON not found: ${RULESET_JSON}" >&2
  exit 1
fi

if [ "$ACTION" = "delete" ]; then
  ID=$(gh api "repos/${REPO}/rulesets" -q '.[] | select(.name=="Main Deployment") | .id' 2>/dev/null || true)
  if [ -z "$ID" ]; then
    echo "No ruleset named 'Main Deployment' found for ${REPO}"
    exit 0
  fi
  echo "Deleting ruleset 'Main Deployment' (id=${ID})..."
  gh api -X DELETE "repos/${REPO}/rulesets/${ID}"
  echo "Done."
  exit 0
fi

# Create: replace placeholder with repo id and POST
PAYLOAD=$(sed "s/\"REPOSITORY_ID_PLACEHOLDER\"/${REPO_ID}/g" "$RULESET_JSON")
EXISTING_ID=$(gh api "repos/${REPO}/rulesets" -q '.[] | select(.name=="Main Deployment") | .id' 2>/dev/null || true)

if [ -n "$EXISTING_ID" ]; then
  echo "Updating existing ruleset 'Main Deployment' (id=${EXISTING_ID}) for ${REPO}..."
  echo "$PAYLOAD" | gh api -X PUT "repos/${REPO}/rulesets/${EXISTING_ID}" --input -
else
  echo "Creating ruleset 'Main Deployment' for ${REPO}..."
  echo "$PAYLOAD" | gh api -X POST "repos/${REPO}/rulesets" --input -
fi
echo "Done. View at: https://github.com/${REPO}/rules"
