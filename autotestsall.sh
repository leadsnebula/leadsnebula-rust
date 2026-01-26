#!/bin/bash
# autotestsall.sh - Fast, efficient test suite runner
# - Single compilation phase (build everything once)
# - Single test execution phase (nextest runs all tests in parallel)
# - Full features enabled (matches CI)
# - Optional Neon ephemeral branch
# Usage: ./autotestsall.sh [--no-neon]

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

# Simple log file
LOG_FILE="/tmp/autotestsall_$(date +%Y%m%d_%H%M%S).log"
exec > >(tee -a "$LOG_FILE")
exec 2>&1

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧪 Running complete test suite"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Log file: $LOG_FILE"
echo ""

# Check for nextest
if ! command -v cargo-nextest >/dev/null 2>&1; then
    echo "⚠️  cargo-nextest not found. Installing..."
    cargo install cargo-nextest --locked
    echo ""
fi

# Load environment variables from .env.local if it exists
if [ -f ".env.local" ]; then
    set -a
    source .env.local
    set +a
fi

# Parse flags
USE_NEON=true
for arg in "$@"; do
    case "$arg" in
        --no-neon)
            USE_NEON=false
            ;;
        *)
            echo "Unknown flag: $arg" >&2
            echo "Usage: $0 [--no-neon]" >&2
            exit 1
            ;;
    esac
done

# === Phase 1: Neon setup (if enabled) ===
BRANCH_NAME=""
if [ "$USE_NEON" = true ]; then
    if [ -z "${NEON_API_KEY:-}" ] || [ -z "${NEON_PROJECT_ID:-}" ]; then
        echo "❌ NEON_API_KEY and NEON_PROJECT_ID required for Neon branch creation" >&2
        echo "   Set them in .env.local or use --no-neon to use existing DATABASE_URL" >&2
        exit 1
    fi
    
    echo "🚀 Creating ephemeral Neon branch..."
    BRANCH_NAME="ci-local-$(date +%s)-$$"
    
    # Clean up old ci-local- branches (prevent limit exceeded)
    OLD_BRANCHES=$(npx --yes neonctl branches list --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" --output json 2>/dev/null | \
        jq -r '.[] | select(.name | startswith("ci-local-")) | .name' 2>/dev/null || true)
    if [ -n "$OLD_BRANCHES" ]; then
        echo "$OLD_BRANCHES" | head -10 | while read -r old_branch; do
            echo "   Deleting old branch: $old_branch"
            npx --yes neonctl branches delete "$old_branch" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" 2>/dev/null || true
        done
    fi
    
    # Create branch
    if ! npx --yes neonctl branches create --name "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" 2>&1; then
        echo "❌ Failed to create Neon branch" >&2
        exit 1
    fi
    
    # Get connection string
    export DATABASE_URL=$(npx --yes neonctl connection-string "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" 2>/dev/null)
    if [ -z "$DATABASE_URL" ]; then
        echo "❌ Failed to get connection string" >&2
        exit 1
    fi
    
    export EPHEMERAL_DB=1
    export TEST_MODE=true
    echo "✅ Using Neon branch: $BRANCH_NAME"
    echo ""
    
    # Wait for DB to be ready
    echo "Waiting for database to be ready..."
    sleep 3
else
    if [ -z "${DATABASE_URL:-}" ]; then
        echo "⚠️  DATABASE_URL not set and --no-neon specified" >&2
        exit 1
    fi
    echo "✅ Using existing DATABASE_URL"
    echo ""
fi

# === Phase 2: Single compilation (all targets, all features) ===
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔨 Compiling workspace (single build)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Use all available CPU cores for compilation
# Unset any restrictive CARGO_BUILD_JOBS from environment
unset CARGO_BUILD_JOBS
export CARGO_BUILD_JOBS=$(nproc)

echo "Using $CARGO_BUILD_JOBS parallel build jobs"
echo ""

if ! cargo build --all-targets --locked --all-features; then
    echo "❌ Build failed"
    exit 1
fi

echo ""
echo "✅ Build complete"
echo ""

# === Phase 2.5: Run migrations (if using Neon) ===
if [ "$USE_NEON" = true ]; then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🗄️  Running database migrations"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    if cargo run --bin run-migrations --locked 2>&1; then
        echo "✅ Migrations applied"
    else
        echo "⚠️  Migration failed (tests will apply migrations automatically via test_helpers)"
    fi
    echo ""
fi

# === Phase 3: Single test run (nextest, all tests) ===
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧪 Running all tests (nextest)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Use all available CPU cores for test execution
TEST_THREADS=$(nproc)
echo "Using $TEST_THREADS parallel test threads"
echo ""

# Single nextest run: all targets, all features, run ignored tests
# Use all CPU cores for maximum parallelism
if ! cargo nextest run --all-targets --locked --all-features --run-ignored all --test-threads "$TEST_THREADS"; then
    TEST_EXIT=1
else
    TEST_EXIT=0
fi

# === Cleanup ===
if [ "$USE_NEON" = true ] && [ -n "$BRANCH_NAME" ]; then
    echo ""
    echo "🧹 Cleaning up Neon branch: $BRANCH_NAME"
    npx --yes neonctl branches delete "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" 2>/dev/null || true
fi

# === Result ===
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ $TEST_EXIT -eq 0 ]; then
    echo "✅ All tests passed!"
else
    echo "❌ Some tests failed (see log: $LOG_FILE)"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

exit $TEST_EXIT
