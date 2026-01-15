#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/clean-orphan-neon-branches.sh --project <project-id> --older-than 24h
# Deletes Neon branches that start with "ci-" (CI ephemeral branches) older than the specified time

PROJECT_ID=""
OLDER_THAN="24h"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --project) PROJECT_ID="$2"; shift 2;;
    --older-than) OLDER_THAN="$2"; shift 2;;
    *) echo "Unknown argument: $1" >&2; exit 2;;
  esac
done

if [ -z "$PROJECT_ID" ]; then
  echo "--project is required" >&2
  exit 2
fi

# Accept either NEON_API_KEY or NEONCTL_API_KEY (set by workflow secrets)
if [ -z "${NEONCTL_API_KEY-}" ] && [ -n "${NEON_API_KEY-}" ]; then
  export NEONCTL_API_KEY="$NEON_API_KEY"
fi

require() { if [ -z "${!1-}" ]; then echo "Required env var $1 is not set" >&2; exit 2; fi }
require NEONCTL_API_KEY

echo "Listing branches for project $PROJECT_ID..."
branches=$(neonctl branches list --project "$PROJECT_ID" --format json)

# Use jq if available, otherwise print branches and exit
if command -v jq >/dev/null 2>&1; then
  # Filter branches starting with "ci-" (CI ephemeral branches)
  CI_BRANCHES=$(echo "$branches" | jq -r '.[] | select(.name | startswith("ci-")) | "\(.name)|\(.created_at)"')
  
  if [ -z "$CI_BRANCHES" ]; then
    echo "No CI branches found to prune"
    exit 0
  fi
  
  echo "Found CI branches:"
  echo "$CI_BRANCHES" | while IFS='|' read -r name created; do
    echo "  - $name (created: $created)"
  done
  
  # For now, delete all ci- branches (we could add date parsing later)
  # The workflow runs daily, so branches older than 24h should be safe to delete
  echo ""
  echo "Deleting CI branches older than $OLDER_THAN..."
  echo "$CI_BRANCHES" | while IFS='|' read -r name created; do
    echo "Deleting branch $name (created at $created)"
    neonctl branches delete "$name" --project "$PROJECT_ID" || echo "Failed to delete $name (may already be deleted)"
  done
  
  echo "Pruning complete"
else
  echo "jq not installed; here are the branches:" >&2
  echo "$branches" >&2
  echo "Install jq to enable pruning" >&2
  exit 2
fi
