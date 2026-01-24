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

# Cleanup function
cleanup() {
    if [ "$USE_NEON" = true ] && [ -n "${BRANCH_NAME:-}" ] && [ -n "${NEON_PROJECT_ID:-}" ]; then
        echo ""
        echo "🧹 Cleaning up Neon branch: $BRANCH_NAME"
        set +e
        echo "Executing: npx --yes neonctl branches delete $BRANCH_NAME --project $NEON_PROJECT_ID --api-key ***"
        npx --yes neonctl branches delete "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "${NEON_API_KEY:-}" 2>/dev/null || true
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
    echo "Executing: npx --yes neonctl branches create --name $BRANCH_NAME --project $NEON_PROJECT_ID --api-key ***"
    if ! npx --yes neonctl branches create --name "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY"; then
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
        CONNECTION=$(npx --yes neonctl connection-string "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" 2>/dev/null) || true
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
    export EPHEMERAL_DB=1
    # Set CI environment variable for test helpers to use CI-specific pool settings
    export CI=1
    # Increase pool size and timeout for CI (Neon free-tier can be very slow)
    export TEST_POOL_MAX_CONNECTIONS="${TEST_POOL_MAX_CONNECTIONS:-50}"
    export TEST_POOL_ACQUIRE_TIMEOUT_SECS="${TEST_POOL_ACQUIRE_TIMEOUT_SECS:-120}"
    echo "✅ DATABASE_URL and EPHEMERAL_DB set (ephemeral branch; no litter in main)"
    echo "   Pool settings: max_connections=$TEST_POOL_MAX_CONNECTIONS, acquire_timeout=${TEST_POOL_ACQUIRE_TIMEOUT_SECS}s"
    
    # Wait a moment for DNS to propagate and database to be fully ready
    echo "Waiting for database to be ready..."
    sleep 3
    
    # Verify connection works before proceeding
    if command -v psql > /dev/null 2>&1; then
        MAX_RETRIES=5
        RETRY_COUNT=0
        while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
            if timeout 10 psql "$CONNECTION" -c "SELECT 1;" > /dev/null 2>&1; then
                echo "✅ Database connection verified"
                break
            else
                RETRY_COUNT=$((RETRY_COUNT + 1))
                if [ $RETRY_COUNT -lt $MAX_RETRIES ]; then
                    echo "   Waiting for database to be ready... (attempt $RETRY_COUNT/$MAX_RETRIES)"
                    sleep 2
                else
                    echo "⚠️  Could not verify database connection, but continuing anyway (tests will retry)"
                fi
            fi
        done
    fi
    
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
    # These tests should be fast, but add timeout as safety measure
    if timeout 60 bash -c "
        if [ \"$HAS_NEXTEST\" = true ]; then
            cargo nextest run --locked --all-features --test integration_health --test integration_routes
        else
            cargo test --locked --all-features --test integration_health --test integration_routes
        fi
    "; then
        echo "✅ Non-database integration tests passed"
    else
        EXIT_CODE=$?
        if [ $EXIT_CODE -eq 124 ]; then
            echo "❌ Non-database integration tests timed out after 60 seconds"
            echo "   This should not happen - these tests don't use a database"
            exit 1
        else
            echo "❌ Non-database integration tests failed"
            exit $EXIT_CODE
        fi
    fi
    echo ""
    
    # 3. Database integration tests (sequential to avoid conflicts)
    echo "3️⃣  Running database integration tests..."
    echo "   ⚠️  These tests can take up to 2 minutes in CI (Neon free-tier is slow)"
    echo "   ⚠️  Watch for migration table race conditions and PoolTimedOut errors"
    
    # Run with timeout wrapper to catch hanging tests
    # Use 150 seconds (2.5 minutes) to allow for slow CI databases
    timeout 150 cargo test --test integration_auth --locked --all-features -- --test-threads=1 2>&1 | tee /tmp/integration_auth_test.log
    TEST_EXIT_CODE=${PIPESTATUS[0]}
    
    # Check for timeout (exit code 124 from timeout command)
    if [ $TEST_EXIT_CODE -eq 124 ]; then
        echo ""
        echo "❌ Database integration tests timed out after 150 seconds"
        echo "   Error: Tests are taking too long, likely due to database slowness or pool exhaustion"
        echo "   Fix: Increase TEST_POOL_MAX_CONNECTIONS and TEST_POOL_ACQUIRE_TIMEOUT_SECS"
        echo "   Or: Database (Neon free-tier) is too slow - consider upgrading or using faster database"
        echo ""
        echo "Last 30 lines of test output:"
        tail -30 /tmp/integration_auth_test.log
        exit 1
    fi
    
    # Check for specific error patterns
    if [ $TEST_EXIT_CODE -ne 0 ]; then
        echo ""
        echo "❌ Database integration tests failed (exit code: $TEST_EXIT_CODE)"
        
        # Check for migration table race conditions
        if grep -q "relation.*_sqlx_migrations.*does not exist\|duplicate key value violates unique constraint.*pg_type_typname_nsp_index" /tmp/integration_auth_test.log; then
            echo ""
            echo "🔍 Detected migration table race condition:"
            echo "   Error: Multiple tests trying to create _sqlx_migrations table concurrently"
            echo "   Fix: Ensure tests run with --test-threads=1 and add retry logic in create_test_pool()"
            echo ""
            grep -E "_sqlx_migrations|pg_type_typname_nsp_index" /tmp/integration_auth_test.log | head -5
        fi
        
        # Check for PoolTimedOut errors
        if grep -q "PoolTimedOut\|has been running for over 60 seconds" /tmp/integration_auth_test.log; then
            echo ""
            echo "🔍 Detected test timeout or pool exhaustion:"
            echo "   Error: Tests taking longer than 60 seconds or database pool exhausted"
            echo "   Fix: Increase TEST_POOL_MAX_CONNECTIONS and TEST_POOL_ACQUIRE_TIMEOUT_SECS"
            echo "   Or: Database is too slow (Neon free-tier can be very slow in CI)"
            echo ""
            grep -E "PoolTimedOut|has been running for over" /tmp/integration_auth_test.log | head -5
        fi
        
        # Show last 20 lines of test output for context
        echo ""
        echo "Last 20 lines of test output:"
        tail -20 /tmp/integration_auth_test.log
        exit $TEST_EXIT_CODE
    fi
    
    echo "✅ Database integration tests passed"
    echo ""
    
    # 4. Publisher CRUD tests (if they exist)
    if cargo test --test integration_publisher_crud --list 2>/dev/null | grep -q "test.*"; then
        echo "4️⃣  Running publisher CRUD tests..."
        # Use 120 seconds timeout for CRUD tests
        if timeout 120 cargo test --test integration_publisher_crud --locked --all-features -- --test-threads=1 2>&1 | tee /tmp/publisher_crud_test.log; then
            echo "✅ Publisher CRUD tests passed"
        else
            EXIT_CODE=${PIPESTATUS[0]}
            if [ $EXIT_CODE -eq 124 ]; then
                echo "❌ Publisher CRUD tests timed out after 120 seconds"
                tail -20 /tmp/publisher_crud_test.log
                exit 1
            else
                echo "❌ Publisher CRUD tests failed"
                tail -20 /tmp/publisher_crud_test.log
                exit $EXIT_CODE
            fi
        fi
        echo ""
    fi
    
    # 5. E2E tests
    if cargo test --test integration_carina_e2e --list 2>/dev/null | grep -q "test.*"; then
        echo "5️⃣  Running E2E tests..."
        echo "   ⚠️  E2E tests can take up to 3 minutes in CI (Neon free-tier is slow)"
        # Use 200 seconds timeout for E2E tests (they're the slowest)
        if timeout 200 cargo test --test integration_carina_e2e --locked --all-features -- --test-threads=1 2>&1 | tee /tmp/e2e_test.log; then
            echo "✅ E2E tests passed"
        else
            EXIT_CODE=${PIPESTATUS[0]}
            if [ $EXIT_CODE -eq 124 ]; then
                echo "❌ E2E tests timed out after 200 seconds"
                tail -20 /tmp/e2e_test.log
                exit 1
            else
                echo "❌ E2E tests failed"
                tail -20 /tmp/e2e_test.log
                exit $EXIT_CODE
            fi
        fi
        echo ""
    fi
    
    # 6. Database-backed library tests (ignored by default)
    echo "6️⃣  Running database-backed library tests (--ignored)..."
    echo "   ⚠️  These tests can take up to 2 minutes in CI (Neon free-tier is slow)"
    
    # Run with timeout wrapper to catch hanging tests
    # Use 180 seconds (3 minutes) for library tests which can be slower
    timeout 180 cargo test --lib --all-features --locked -- --ignored 2>&1 | tee /tmp/lib_tests.log
    TEST_EXIT_CODE=${PIPESTATUS[0]}
    
    # Check for timeout (exit code 124 from timeout command)
    if [ $TEST_EXIT_CODE -eq 124 ]; then
        echo ""
        echo "❌ Database-backed library tests timed out after 180 seconds"
        echo "   Error: Tests are taking too long, likely due to database slowness or pool exhaustion"
        echo "   Fix: Increase TEST_POOL_MAX_CONNECTIONS and TEST_POOL_ACQUIRE_TIMEOUT_SECS"
        echo "   Or: Database (Neon free-tier) is too slow - consider upgrading or using faster database"
        echo ""
        echo "Last 30 lines of test output:"
        tail -30 /tmp/lib_tests.log
        exit 1
    fi
    
    # Check for specific error patterns
    if [ $TEST_EXIT_CODE -ne 0 ]; then
        echo ""
        echo "❌ Database-backed library tests failed (exit code: $TEST_EXIT_CODE)"
        
        # Check for migration table race conditions
        if grep -q "relation.*_sqlx_migrations.*does not exist\|duplicate key value violates unique constraint.*pg_type_typname_nsp_index" /tmp/lib_tests.log; then
            echo ""
            echo "🔍 Detected migration table race condition:"
            echo "   Error: Multiple tests trying to create _sqlx_migrations table concurrently"
            echo "   Fix: Ensure tests run with --test-threads=1 and add retry logic in create_test_pool()"
            echo ""
            grep -E "_sqlx_migrations|pg_type_typname_nsp_index" /tmp/lib_tests.log | head -5
        fi
        
        # Check for PoolTimedOut errors
        if grep -q "PoolTimedOut\|has been running for over 60 seconds" /tmp/lib_tests.log; then
            echo ""
            echo "🔍 Detected test timeout or pool exhaustion:"
            echo "   Error: Tests taking longer than 60 seconds or database pool exhausted"
            echo "   Fix: Increase TEST_POOL_MAX_CONNECTIONS and TEST_POOL_ACQUIRE_TIMEOUT_SECS"
            echo "   Or: Database is too slow (Neon free-tier can be very slow in CI)"
            echo ""
            grep -E "PoolTimedOut|has been running for over" /tmp/lib_tests.log | head -5
        fi
        
        # Show last 20 lines of test output for context
        echo ""
        echo "Last 20 lines of test output:"
        tail -20 /tmp/lib_tests.log
        exit $TEST_EXIT_CODE
    fi
    
    echo "✅ Database-backed library tests passed"
    echo ""
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All tests passed!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Cleanup happens via trap
