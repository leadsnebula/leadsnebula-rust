#!/bin/bash
# Check for pending database migrations
# This script verifies that all migration files have been applied to the database

set -e

echo "🔍 Checking for pending migrations..."

# Load environment
if [ -f .env.local ]; then
    export $(cat .env.local | grep -v '^#' | xargs)
fi

# Check if DATABASE_URL is set
if [ -z "$DATABASE_URL" ]; then
    echo "❌ DATABASE_URL not set. Cannot check migrations."
    echo "   Set DATABASE_URL or ensure .env.local is configured"
    exit 1
fi

# Get list of all migration files
MIGRATION_FILES=$(find migrations -name "*.sql" -type f | sort | xargs -n1 basename)
MIGRATION_COUNT=$(echo "$MIGRATION_FILES" | wc -l)

echo "📁 Found $MIGRATION_COUNT migration files"

# Check applied migrations in database
echo "🔌 Connecting to database..."
APPLIED_MIGRATIONS=$(psql "$DATABASE_URL" -t -c "SELECT version FROM _sqlx_migrations WHERE success = true ORDER BY version;" 2>/dev/null || echo "")

if [ -z "$APPLIED_MIGRATIONS" ]; then
    echo "⚠️  Could not query database migrations table"
    echo "   This might mean migrations haven't been run yet"
    echo ""
    echo "   To apply migrations, run:"
    echo "   cargo run --bin run-migrations"
    exit 1
fi

APPLIED_COUNT=$(echo "$APPLIED_MIGRATIONS" | grep -v '^$' | wc -l)
echo "✅ Found $APPLIED_COUNT applied migrations in database"

# Extract version numbers from migration files
FILE_VERSIONS=$(echo "$MIGRATION_FILES" | sed 's/^\([0-9]*\)_.*/\1/')
DB_VERSIONS=$(echo "$APPLIED_MIGRATIONS" | tr -d ' ' | grep -v '^$')

# Find missing migrations
MISSING=""
for version in $FILE_VERSIONS; do
    if ! echo "$DB_VERSIONS" | grep -q "^${version}$"; then
        MISSING="$MISSING $version"
    fi
done

if [ -n "$MISSING" ]; then
    echo ""
    echo "❌ PENDING MIGRATIONS DETECTED:"
    for version in $MISSING; do
        FILE=$(find migrations -name "${version}_*.sql" -type f | xargs -n1 basename)
        echo "   - $FILE (version: $version)"
    done
    echo ""
    echo "⚠️  Run migrations to apply:"
    echo "   cargo run --bin run-migrations"
    exit 1
else
    echo ""
    echo "✅ All migrations are applied!"
    echo "   Database is up to date with migration files"
    exit 0
fi
