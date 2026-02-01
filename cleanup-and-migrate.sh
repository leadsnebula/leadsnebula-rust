#!/bin/bash
# Cleanup dev database and run migrations
# This script:
# 1. Runs cleanup_dev_db.sql to remove test junk records
# 2. Runs migrations to ensure database is up to date

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

# Load environment from .env.local if it exists
if [ -f ".env.local" ]; then
    echo "Loading environment from .env.local..."
    set -a
    source .env.local
    set +a
fi

# Check if DATABASE_URL is set
if [ -z "${DATABASE_URL:-}" ]; then
    echo "❌ DATABASE_URL not set. Cannot run cleanup."
    echo "   Set DATABASE_URL or ensure .env.local is configured"
    exit 1
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧹 Cleaning up dev database..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Database: ${DATABASE_URL%%\?*}"  # Show URL without query params
echo ""
echo "⚠️  This will delete all records except:"
echo "   - Publisher: Only Solar (0d2a06f2-af57-40c8-b3c7-b61fc7621de6)"
echo "   - Buyers: Solar Test Buyer 1 & 2"
echo "   - Campaigns: Solar Test Campaign & Solar Test Campaign 2"
echo "   - Ping Trees: Solar Test Ping Tree 1 & 2"
echo ""

# Ask for confirmation
read -p "Continue? (yes/no): " confirm
if [ "$confirm" != "yes" ]; then
    echo "❌ Cleanup cancelled"
    exit 1
fi

echo ""
echo "Running cleanup script..."
if psql "$DATABASE_URL" -f cleanup_dev_db.sql; then
    echo "✅ Cleanup completed successfully"
else
    echo "❌ Cleanup failed"
    exit 1
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔄 Running migrations..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

if cargo run --bin run-migrations; then
    echo "✅ Migrations completed successfully"
else
    echo "❌ Migrations failed"
    exit 1
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All done! Database cleaned and migrations applied."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
