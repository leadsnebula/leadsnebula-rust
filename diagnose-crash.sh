#!/bin/bash
# Diagnostic script for WSL crash during compilation
# Runs the failing test with verbose output and memory monitoring
#
# Usage:
#   ./diagnose-crash.sh
#   OR: bash diagnose-crash.sh (if direct execution fails in WSL)
#
# This script:
# 1. Runs the integration_publisher_crud test with verbose output
# 2. Monitors memory usage at 100ms intervals
# 3. Captures backtrace for debugging

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

# Source common setup
source "$SCRIPT_DIR/common_setup.sh"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔍 WSL Crash Diagnostic"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "This script runs the failing test with detailed diagnostics:"
echo "  - RUST_BACKTRACE=1 for full backtraces"
echo "  - CARGO_BUILD_JOBS=1 for single-threaded compilation"
echo "  - --verbose for detailed cargo output"
echo "  - Memory monitoring at 100ms intervals"
echo ""

# Setup database if needed
if [ -z "${DATABASE_URL:-}" ]; then
    if [ "$IS_WSL" = "true" ]; then
        echo "⚠️  WSL detected: Creating ephemeral Neon branch for diagnostics..."
        trap cleanup_neon_branch EXIT
        setup_neon_branch
        export DATABASE_URL
        export EPHEMERAL_DB=1
    else
        echo "❌ DATABASE_URL not set and not in WSL" >&2
        exit 1
    fi
fi

# Ensure EPHEMERAL_DB is set
export EPHEMERAL_DB=1

# Set diagnostic environment
export RUST_BACKTRACE=1
export CARGO_BUILD_JOBS=1  # Single-threaded to isolate the crash
export CARGO_INCREMENTAL=0

# Start memory monitoring in background
MEM_LOG="/tmp/wsl_crash_mem.log"
echo "📊 Starting memory monitoring (logging to $MEM_LOG)..."
echo "   Run 'tail -f $MEM_LOG' in another terminal to watch memory"
free -h -s 0.1 > "$MEM_LOG" 2>&1 &
MEM_PID=$!

# Cleanup function
cleanup() {
    echo ""
    echo "🧹 Stopping memory monitoring..."
    kill $MEM_PID 2>/dev/null || true
    wait $MEM_PID 2>/dev/null || true
    echo "✅ Memory log saved to: $MEM_LOG"
    echo "   Analyze with: grep -E 'available|Mem:' $MEM_LOG | tail -20"
}

trap cleanup EXIT INT TERM

echo "🧪 Running diagnostic test..."
echo "   Test: integration_publisher_crud"
echo "   Features: webauthn tracing (trimmed to reduce bloat)"
echo "   Build jobs: 1 (single-threaded for isolation)"
echo ""

# Run the test with verbose output
# Use same feature flags as test-db.sh (but trimmed: no sentry/profiling)
cargo test --verbose --test integration_publisher_crud \
    --no-default-features \
    --features "webauthn tracing" \
    --locked \
    -- --test-threads=1 2>&1 | tee /tmp/wsl_crash_diagnostic.log

TEST_EXIT_CODE=${PIPESTATUS[0]}

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ $TEST_EXIT_CODE -eq 0 ]; then
    echo "✅ Test completed successfully"
else
    echo "❌ Test failed with exit code: $TEST_EXIT_CODE"
    echo ""
    echo "📋 Last 30 lines of output:"
    tail -30 /tmp/wsl_crash_diagnostic.log
    echo ""
    echo "📊 Memory usage during crash (last 20 samples):"
    grep -E 'available|Mem:' "$MEM_LOG" | tail -20 || echo "   (memory log not available)"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "📁 Diagnostic files:"
echo "   - Test output: /tmp/wsl_crash_diagnostic.log"
echo "   - Memory log: $MEM_LOG"
echo ""
echo "💡 To analyze memory peaks:"
echo "   grep -E 'available' $MEM_LOG | awk '{print \$7}' | sort -n | head -5"
echo "   (Shows lowest available memory values - peak usage)"
echo ""

exit $TEST_EXIT_CODE
