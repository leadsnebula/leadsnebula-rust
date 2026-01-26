#!/bin/bash
# One-time script to manually run the fix migration on existing databases
# This fixes inconsistent database state from partial migrations
#
# Usage:
#   ./run-fix-migration.sh [DATABASE_URL]
#
# If DATABASE_URL is not provided, it will be read from environment or .env.local

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

# Get DATABASE_URL from argument, environment, or .env.local
if [ $# -ge 1 ]; then
    export DATABASE_URL="$1"
else
    if [ -n "${DATABASE_URL:-}" ]; then
        echo "Using DATABASE_URL from environment"
    else
        if [ -f ".env.local" ]; then
            echo "Loading DATABASE_URL from .env.local"
            set -a
            source .env.local
            set +a
        else
            echo "❌ DATABASE_URL not provided and not found in environment or .env.local" >&2
            echo "Usage: $0 [DATABASE_URL]" >&2
            exit 1
        fi
    fi
fi

if [ -z "${DATABASE_URL:-}" ]; then
    echo "❌ DATABASE_URL is empty" >&2
    exit 1
fi

# Check for --yes flag to skip confirmation
SKIP_CONFIRM=false
if [ "$1" = "--yes" ] || [ "$1" = "-y" ]; then
    SKIP_CONFIRM=true
    # Remove the flag from arguments so DATABASE_URL parsing works correctly
    shift
    if [ $# -ge 1 ]; then
        export DATABASE_URL="$1"
    fi
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔧 Running fix migration on existing database"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Database: ${DATABASE_URL%%\?*}"  # Show URL without query params
echo ""
echo "⚠️  This will:"
echo "   - Fix inconsistent webauthn_credentials table state"
echo "   - Fix inconsistent user_otp_settings table state"
echo "   - Clean up orphaned migration records"
echo ""

if [ "$SKIP_CONFIRM" = "false" ]; then
    read -p "Continue? (y/N) " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Cancelled"
        exit 0
    fi
fi

echo ""
echo "Running fix migration: 20260115000000_fix_partial_migrations.sql"
echo ""

# Run the fix migration using cargo run-migrations
# The migration file will be executed by sqlx migrate
if cargo run --bin run-migrations --locked 2>&1; then
    echo ""
    echo "✅ Fix migration completed successfully"
    echo ""
    echo "Verifying database state..."
    
    # Verify the fix worked
    if psql "$DATABASE_URL" -c "\d webauthn_credentials" > /dev/null 2>&1; then
        PLATFORM_COL=$(psql "$DATABASE_URL" -t -c "SELECT COUNT(*) FROM information_schema.columns WHERE table_name = 'webauthn_credentials' AND column_name = 'platform_user_id'" 2>/dev/null | tr -d ' ')
        INSTANCE_COL=$(psql "$DATABASE_URL" -t -c "SELECT COUNT(*) FROM information_schema.columns WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'" 2>/dev/null | tr -d ' ')
        
        if [ "$PLATFORM_COL" = "1" ] && [ "$INSTANCE_COL" = "1" ]; then
            echo "✅ webauthn_credentials table has all required columns"
        else
            echo "⚠️  webauthn_credentials table may still have issues"
            echo "   platform_user_id: $PLATFORM_COL (expected: 1)"
            echo "   instance_user_id: $INSTANCE_COL (expected: 1)"
        fi
    else
        echo "⚠️  Could not verify webauthn_credentials table (psql may not be available)"
    fi
    
    exit 0
else
    echo ""
    echo "❌ Fix migration failed"
    echo ""
    echo "If the migration was already applied, this is expected."
    echo "The migration is idempotent and safe to run multiple times."
    exit 1
fi

