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

# Use NEONCTL_CMD if set (from workflow), otherwise use npx --yes neonctl
NEONCTL_CMD="${NEONCTL_CMD:-npx --yes neonctl}"

echo "Listing branches for project $PROJECT_ID..."
# Redirect stderr to suppress npm/npx warnings, capture stdout for JSON
# Use --output json (not --format json) to match other scripts
branches_output=$($NEONCTL_CMD branches list --project "$PROJECT_ID" --api-key "$NEONCTL_API_KEY" --output json 2>/dev/null || true)

# Extract JSON from output (in case there's non-JSON content mixed in from npm/npx)
# Remove all lines before the first line that starts with '[' or '{'
branches=$(echo "$branches_output" | sed -n '/^\s*[\[{]/,$p')

# If sed didn't find JSON start, try the whole output
if [ -z "$branches" ] || ! echo "$branches" | jq empty 2>/dev/null; then
  # Try the original output as-is
  branches="$branches_output"
fi

# Validate that we have valid JSON before proceeding
if ! echo "$branches" | jq empty 2>/dev/null; then
  echo "Error: Failed to get valid JSON from neonctl output" >&2
  echo "Raw output (first 500 chars):" >&2
  echo "$branches_output" | head -c 500 >&2
  echo "" >&2
  exit 1
fi

# Parse OLDER_THAN (e.g. 24h, 12h, 48h) into seconds
OLD_SECONDS=86400
if [[ "$OLDER_THAN" =~ ^([0-9]+)h$ ]]; then
  OLD_SECONDS=$((${BASH_REMATCH[1]} * 3600))
elif [[ "$OLDER_THAN" =~ ^([0-9]+)d$ ]]; then
  OLD_SECONDS=$((${BASH_REMATCH[1]} * 86400))
fi
NOW_EPOCH=$(date +%s)

# Use jq if available, otherwise print branches and exit
if command -v jq >/dev/null 2>&1; then
  # Filter branches starting with "ci-" (CI ephemeral branches)
  CI_BRANCHES=$(echo "$branches" | jq -r '.[] | select(.name | startswith("ci-")) | "\(.name)|\(.created_at)"')
  
  if [ -z "$CI_BRANCHES" ]; then
    echo "No CI branches found to prune"
    exit 0
  fi
  
  echo "Found CI branches (deleting only if older than $OLDER_THAN = ${OLD_SECONDS}s):"
  echo "$CI_BRANCHES" | while IFS='|' read -r name created; do
    echo "  - $name (created: $created)"
  done
  
  echo ""
  echo "Deleting CI branches older than $OLDER_THAN..."
  while IFS='|' read -r name created; do
    # Parse created_at (ISO 8601) to epoch; skip if unparseable
    CREATED_EPOCH=$(date -d "$created" +%s 2>/dev/null) || continue
    AGE=$((NOW_EPOCH - CREATED_EPOCH))
    if [ "$AGE" -ge "$OLD_SECONDS" ]; then
      echo "Deleting branch $name (created $created, age ${AGE}s)"
      $NEONCTL_CMD branches delete "$name" --project "$PROJECT_ID" --api-key "$NEONCTL_API_KEY" || echo "Failed to delete $name (may already be deleted)"
    else
      echo "Skipping $name (age ${AGE}s < ${OLD_SECONDS}s)"
    fi
  done <<< "$CI_BRANCHES"
  
  echo "Pruning complete"
else
  echo "jq not installed; here are the branches:" >&2
  echo "$branches" >&2
  echo "Install jq to enable pruning" >&2
  exit 2
fi
