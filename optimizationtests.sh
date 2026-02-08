#!/bin/bash
# optimizationtests.sh - Run optimization/performance tests only
# These tests verify: parallel query execution, cache hit/miss, write-behind queue,
# SSM key caching. Requires RUN_HEAVY_TESTS, DATABASE_URL, and optionally Redis/SSM.
# Usage: ./optimizationtests.sh [--no-neon]

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

LOG_FILE="/tmp/optimizationtests_$(date +%Y%m%d_%H%M%S).log"
exec > >(tee -a "$LOG_FILE")
exec 2>&1

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📊 Running optimization tests"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Log file: $LOG_FILE"
echo ""

if ! command -v cargo-nextest >/dev/null 2>&1; then
    echo "⚠️  cargo-nextest not found. Installing..."
    cargo install cargo-nextest --locked
    echo ""
fi

if [ -f ".env.local" ]; then
    set -a
    source .env.local
    set +a
fi

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

# Optimization tests require RUN_HEAVY_TESTS
export RUN_HEAVY_TESTS=true

# === Phase 1: Neon setup (if enabled) ===
BRANCH_NAME=""
if [ "$USE_NEON" = true ]; then
    if [ -z "${NEON_API_KEY:-}" ] || [ -z "${NEON_PROJECT_ID:-}" ]; then
        echo "❌ NEON_API_KEY and NEON_PROJECT_ID required" >&2
        echo "   Set them in .env.local or use --no-neon" >&2
        exit 1
    fi

    echo "🚀 Creating ephemeral Neon branch..."
    BRANCH_NAME="ci-opt-$(date +%s)-$$"

    if ! npx --yes neonctl branches create --name "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" 2>&1; then
        echo "❌ Failed to create Neon branch" >&2
        exit 1
    fi

    export DATABASE_URL=$(npx --yes neonctl connection-string "$BRANCH_NAME" --project "$NEON_PROJECT_ID" --api-key "$NEON_API_KEY" 2>/dev/null)
    if [ -z "$DATABASE_URL" ]; then
        echo "❌ Failed to get connection string" >&2
        exit 1
    fi
    # Per Neon docs: sslnegotiation=direct reduces connection latency
    if [[ "$DATABASE_URL" == *"?"* ]]; then
        export DATABASE_URL="${DATABASE_URL}&sslnegotiation=direct"
    else
        export DATABASE_URL="${DATABASE_URL}?sslnegotiation=direct"
    fi

    export EPHEMERAL_DB=1
    export TEST_MODE=true
    echo "✅ Using Neon branch: $BRANCH_NAME"
    echo ""
    sleep 3
else
    if [ -z "${DATABASE_URL:-}" ]; then
        echo "⚠️  DATABASE_URL not set and --no-neon specified" >&2
        exit 1
    fi
    echo "✅ Using existing DATABASE_URL"
    echo ""
fi

# === Phase 2: Build ===
echo "🔨 Building..."
if ! cargo build --all-targets --locked --all-features; then
    echo "❌ Build failed"
    exit 1
fi
echo ""

# === Phase 3: Migrations ===
if [ "$USE_NEON" = true ]; then
    echo "🗄️  Running migrations..."
    if ! cargo run --bin run-migrations --locked 2>&1; then
        echo "⚠️  Migration failed (test_helpers will apply)"
    fi
    echo ""
fi

# === Phase 4: Warm up Neon ===
if [ "$USE_NEON" = true ] && [ -n "${DATABASE_URL:-}" ]; then
    echo "🔥 Warming up Neon compute..."
    for i in 1 2 3; do
        cargo run --bin run-migrations --locked 2>/dev/null || true
        sleep 2
    done
    sleep 3
    echo "✅ Warm-up complete"
    echo ""
fi

# === Phase 5: Run optimization tests only ===
echo "🧪 Running optimization tests (sequential)..."
echo ""

export WRITE_BEHIND_FLUSH_TIMEOUT_SECS="${WRITE_BEHIND_FLUSH_TIMEOUT_SECS:-60}"

if ! cargo nextest run --package leadsnebula_core --lib --locked --all-features \
    --run-ignored only --test-threads 1 --profile ci \
    -E 'test(optimization_tests)'; then
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

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ $TEST_EXIT -eq 0 ]; then
    echo "✅ Optimization tests passed!"
else
    echo "❌ Some tests failed (see log: $LOG_FILE)"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

exit $TEST_EXIT
