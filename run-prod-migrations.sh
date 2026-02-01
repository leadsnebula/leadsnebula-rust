#!/bin/bash
# Run SQLx migrations against the production database.
# Use this when syncing dev schema to prod before merging dev → main.
#
# Usage:
#   ./run-prod-migrations.sh
#   # (uses DATABASE_URL from env, or pass URL as first arg)
#
#   DATABASE_URL='postgresql://...prod...' ./run-prod-migrations.sh
#   ./run-prod-migrations.sh 'postgresql://user:pass@host/db?sslmode=require'
#
# Prod URL (strip "psql " and quotes if copying from psql invocations):
#   postgresql://neondb_owner:npg_L4nuGPSchU5z@ep-fragrant-bar-aht6x6lv-pooler.c-3.us-east-1.aws.neon.tech/neondb?sslmode=require&channel_binding=require

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

if [ $# -ge 1 ]; then
  export DATABASE_URL="$1"
  echo "Using DATABASE_URL from argument"
elif [ -n "${DATABASE_URL:-}" ]; then
  echo "Using DATABASE_URL from environment"
else
  echo "❌ DATABASE_URL not set. Pass prod URL as first arg or set DATABASE_URL." >&2
  echo "Example: $0 'postgresql://user:pass@host/db?sslmode=require'" >&2
  exit 1
fi

# Strip "psql " prefix and surrounding quotes if present
DATABASE_URL="${DATABASE_URL#'psql '}"
DATABASE_URL="${DATABASE_URL%\'}"
DATABASE_URL="${DATABASE_URL#\'}"
export DATABASE_URL

echo "Running migrations against: ${DATABASE_URL%%\?*}"
cargo run --bin run-migrations --locked
