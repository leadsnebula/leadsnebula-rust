#!/bin/bash
# Auto-test script with ephemeral Neon database
# Creates an ephemeral Neon branch, runs all tests, then cleans up
#
# Usage:
#   ./autotests.sh              # Full test suite with ephemeral Neon DB
#   ./autotests.sh --no-neon    # Run tests without creating Neon branch (uses existing DATABASE_URL)
#   ./autotests.sh --unit-only  # Run only unit tests (no DB needed)
#
# Requirements:
#   - NEON_API_KEY and NEON_PROJECT_ID in .env.local or environment
#   - npx (for neonctl)
#   - cargo-nextest (optional, for faster parallel tests)

set -euo pipefail

# Parse flags
USE_NEON=true
UNIT_ONLY=false
for arg in "$@"; do
    case "$arg" in
        --no-neon)
            USE_NEON=false
            ;;
        --unit-only)
            UNIT_ONLY=true
            USE_NEON=false
            ;;
        *)
            echo "Unknown flag: $arg" >&2
            echo "Usage: $0 [--no-neon] [--unit-only]" >&2
            exit 1
            ;;
    esac
done

# Load environment variables from .env.local if it exists
if [ -f ".env.local" ]; then
    echo "📋 Loading environment from .env.local..."
    export $(grep -v '^#' .env.local | grep -v '^$' | xargs)
fi

# Check for required Neon credentials if using Neon
if [ "$USE_NEON" = true ]; then
    if [ -z "${NEON_API_KEY:-}" ]; then
        echo "❌ NEON_API_KEY is required for ephemeral Neon branch creation" >&2
        echo "   Set it in .env.local or environment" >&2
        exit 1
    fi
    
    if [ -z "${NEON_PROJECT_ID:-}" ]; then
        echo "❌ NEON_PROJECT_ID is required for ephemeral Neon branch creation" >&2
        echo "   Set it in .env.local or environment" >&2
        exit 1
    fi
    
    # Check for npx
    if ! command -v npx > /dev/null 2>&1; then
        echo "❌ npx is required for neonctl (install: npm install -g npm)" >&2
        exit 1
    fi
fi

# Generate unique branch name
BRANCH_NAME="ci-local-$(date +%s)-$$"
export NEONCTL_API_KEY="${NEON_API_KEY:-}"

# Cleanup function
cleanup() {
    if [ "$USE_NEON" = true ] && [ -n "${BRANCH_NAME:-}" ] && [ -n "${NEON_PROJECT_ID:-}" ]; then
        echo ""
        echo "🧹 Cleaning up Neon branch: $BRANCH_NAME"
        set +e
        echo "Executing: npx --yes neonctl branches delete $BRANCH_NAME --project $NEON_PROJECT_ID"
        npx --yes neonctl branches delete "$BRANCH_NAME" --project "$NEON_PROJECT_ID" 2>/dev/null || true
        set -e
        echo "✅ Cleanup complete"
    fi
}
trap cleanup EXIT

# Create ephemeral Neon branch if requested
if [ "$USE_NEON" = true ]; then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🚀 Creating ephemeral Neon branch for testing"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Branch name: $BRANCH_NAME"
    echo "Project ID: $NEON_PROJECT_ID"
    echo ""
    
    # Create branch (using --name flag like scripts/test_neon_ephemeral.sh)
    echo "Creating branch..."
    echo "Executing: npx --yes neonctl branches create --name $BRANCH_NAME --project $NEON_PROJECT_ID"
    if ! npx --yes neonctl branches create --name "$BRANCH_NAME" --project "$NEON_PROJECT_ID"; then
        echo "❌ Failed to create Neon branch $BRANCH_NAME" >&2
        exit 1
    fi
    echo "✅ Branch created: $BRANCH_NAME"
    
    # Get connection string (branch should be ready immediately after create)
    echo "Fetching connection string..."
    MAX_RETRIES=5
    RETRY_COUNT=0
    CONNECTION=""
    
    while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
        CONNECTION=$(npx --yes neonctl connection-string "$BRANCH_NAME" --project "$NEON_PROJECT_ID" 2>/dev/null) || true
        if [ -n "$CONNECTION" ]; then
            break
        fi
        RETRY_COUNT=$((RETRY_COUNT + 1))
        if [ $RETRY_COUNT -lt $MAX_RETRIES ]; then
            echo "   Waiting for branch to be ready... (attempt $RETRY_COUNT/$MAX_RETRIES)"
            sleep 2
        fi
    done
    
    if [ -z "$CONNECTION" ]; then
        echo "❌ Failed to get connection string for $BRANCH_NAME after $MAX_RETRIES attempts" >&2
        exit 1
    fi
    
    export DATABASE_URL="$CONNECTION"
    echo "✅ DATABASE_URL set"
    
    # Run migrations on the ephemeral branch
    echo "Running database migrations..."
    # Set TEST_MODE for ephemeral test databases to enable automatic cleanup of inconsistent migrations
    export TEST_MODE=true
    if cargo run --release --bin run-migrations --locked 2>/dev/null; then
        echo "✅ Migrations applied successfully"
    else
        echo "⚠️  Migration binary not built or failed (tests will apply migrations automatically)"
        echo "   Building migration binary..."
        if cargo build --release --bin run-migrations --locked 2>/dev/null; then
            echo "   Running migrations..."
            export TEST_MODE=true
            cargo run --release --bin run-migrations --locked || {
                echo "⚠️  Manual migration failed, but sqlx::test will apply migrations automatically"
            }
        else
            echo "⚠️  Could not build migration binary (tests will apply migrations via sqlx::test)"
        fi
    fi
    echo ""
elif [ "$UNIT_ONLY" = false ] && [ -z "${DATABASE_URL:-}" ]; then
    echo "⚠️  DATABASE_URL not set and --no-neon specified"
    echo "   Running unit tests only..."
    UNIT_ONLY=true
fi

# Run tests
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧪 Running test suite"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

if [ "$UNIT_ONLY" = true ]; then
    echo "Running unit tests only (no database required)..."
    echo ""
    
    if command -v cargo-nextest > /dev/null 2>&1; then
        echo "Using cargo-nextest for parallel execution..."
        cargo nextest run --lib --locked --all-features
    else
        echo "Using cargo test (install cargo-nextest for faster tests)..."
        cargo test --lib --locked --all-features
    fi
else
    echo "Running full test suite (unit + integration + E2E)..."
    echo ""
    
    # Check for cargo-nextest
    HAS_NEXTEST=false
    if command -v cargo-nextest > /dev/null 2>&1; then
        HAS_NEXTEST=true
        echo "✅ Using cargo-nextest for faster parallel execution"
    else
        echo "⚠️  cargo-nextest not installed (install: cargo install cargo-nextest)"
        echo "   Using cargo test (slower, but works)"
    fi
    echo ""
    
    # 1. Unit tests (fast, parallel)
    echo "1️⃣  Running unit tests..."
    if [ "$HAS_NEXTEST" = true ]; then
        cargo nextest run --lib --locked --all-features
    else
        cargo test --lib --locked --all-features
    fi
    echo "✅ Unit tests passed"
    echo ""
    
    # 2. Non-database integration tests
    echo "2️⃣  Running non-database integration tests..."
    if [ "$HAS_NEXTEST" = true ]; then
        cargo nextest run --locked --all-features --test integration_health --test integration_routes
    else
        cargo test --locked --all-features --test integration_health --test integration_routes
    fi
    echo "✅ Non-database integration tests passed"
    echo ""
    
    # 3. Database integration tests (sequential to avoid conflicts)
    echo "3️⃣  Running database integration tests..."
    cargo test --test integration_auth --locked --all-features -- --test-threads=1
    echo "✅ Database integration tests passed"
    echo ""
    
    # 4. Publisher CRUD tests (if they exist)
    if cargo test --test integration_publisher_crud --list 2>/dev/null | grep -q "test.*"; then
        echo "4️⃣  Running publisher CRUD tests..."
        cargo test --test integration_publisher_crud --locked --all-features -- --test-threads=1
        echo "✅ Publisher CRUD tests passed"
        echo ""
    fi
    
    # 5. E2E tests
    if cargo test --test integration_carina_e2e --list 2>/dev/null | grep -q "test.*"; then
        echo "5️⃣  Running E2E tests..."
        cargo test --test integration_carina_e2e --locked --all-features -- --test-threads=1
        echo "✅ E2E tests passed"
        echo ""
    fi
    
    # 6. Database-backed library tests (ignored by default)
    echo "6️⃣  Running database-backed library tests (--ignored)..."
    cargo test --lib --all-features --locked -- --ignored
    echo "✅ Database-backed library tests passed"
    echo ""
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All tests passed!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Cleanup happens via trap
