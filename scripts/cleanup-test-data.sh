#!/bin/bash
# Cleanup script to remove all test data from main database
# WARNING: This will DELETE all test records from publishers, buyers, campaigns, and ping_trees
# Run this ONLY on your main/dev database, NOT on production
#
# Usage:
#   ./scripts/cleanup-test-data.sh
#   Or: ./scripts/cleanup-test-data.sh --dry-run  (show what would be deleted)

set -euo pipefail

DRY_RUN=false
if [ "${1:-}" = "--dry-run" ]; then
    DRY_RUN=true
    echo "🔍 DRY RUN MODE - No data will be deleted"
fi

# Load .env.local if it exists
if [ -f ".env.local" ]; then
    echo "📋 Loading environment from .env.local..."
    export $(grep -v '^#' .env.local | grep -v '^$' | xargs)
fi

if [ -z "${DATABASE_URL:-}" ]; then
    echo "❌ DATABASE_URL is not set" >&2
    echo "   Set it in .env.local or export it in your shell" >&2
    exit 1
fi

# Safety check: refuse to run on production-like URLs
if echo "$DATABASE_URL" | grep -qiE "(prod|production|main.*prod)"; then
    echo "❌ ERROR: DATABASE_URL looks like production!" >&2
    echo "   This script is for dev/main databases only." >&2
    echo "   Current DATABASE_URL: ${DATABASE_URL:0:50}..." >&2
    exit 1
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧹 Cleaning up test data from database"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "DATABASE_URL: ${DATABASE_URL:0:60}..."
echo ""

if [ "$DRY_RUN" = true ]; then
    echo "🔍 DRY RUN - Would execute: scripts/cleanup-test-data.sql"
    echo ""
    echo "To actually delete test data, run:"
    echo "  ./scripts/cleanup-test-data.sh"
    exit 0
fi

# Confirm before proceeding
echo "⚠️  WARNING: This will DELETE all test data from:"
echo "   - publishers (test names/emails)"
echo "   - buyers (test names)"
echo "   - campaigns (test names)"
echo "   - ping_trees (test names)"
echo "   - ping_tree_campaigns (orphaned)"
echo "   - leads (test data)"
echo "   - instance_users (test emails)"
echo "   - instances (orphaned)"
echo ""
read -p "Continue? (yes/no): " confirm
if [ "$confirm" != "yes" ]; then
    echo "❌ Aborted"
    exit 1
fi

echo ""
echo "🗑️  Deleting test data..."

# Run the SQL cleanup script
if command -v psql > /dev/null 2>&1; then
    psql "$DATABASE_URL" -f scripts/cleanup-test-data.sql
elif command -v sqlx > /dev/null 2>&1; then
    # Alternative: use sqlx-cli if psql is not available
    echo "Using sqlx-cli..."
    sqlx database execute "$DATABASE_URL" --file scripts/cleanup-test-data.sql
else
    echo "❌ Neither psql nor sqlx-cli found. Install one to run cleanup." >&2
    exit 1
fi

echo ""
echo "✅ Cleanup complete!"
echo ""
echo "Remaining record counts shown above."
