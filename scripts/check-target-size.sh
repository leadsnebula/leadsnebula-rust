#!/bin/bash
# scripts/check-target-size.sh
# Monitor target/ directory size and warn if it gets too large
#
# Usage:
#   ./scripts/check-target-size.sh          # Check and warn if > 50GB
#   ./scripts/check-target-size.sh 30      # Custom threshold in GB

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

THRESHOLD_GB="${1:-50}"

# Safety check: Ensure we're in a Rust project
if [ ! -f "Cargo.toml" ]; then
    echo "❌ ERROR: Not in a Rust project (Cargo.toml not found)" >&2
    exit 1
fi

# Check if target/ exists
if [ ! -d "target" ]; then
    echo "✅ target/ directory does not exist (nothing to check)"
    exit 0
fi

# Get size in MB and GB
SIZE_MB=$(du -sm target/ 2>/dev/null | cut -f1 || echo "0")
SIZE_GB=$((SIZE_MB / 1024))
SIZE_HUMAN=$(du -sh target/ 2>/dev/null | cut -f1 || echo "0")

echo "target/ directory size: $SIZE_HUMAN ($SIZE_MB MB, ~$SIZE_GB GB)"

# Check threshold
if [ "$SIZE_GB" -gt "$THRESHOLD_GB" ]; then
    echo ""
    echo "⚠️  WARNING: target/ directory is very large (> ${THRESHOLD_GB}GB)"
    echo "   This can slow down builds and waste disk space."
    echo ""
    echo "   To clean build artifacts, run:"
    echo "     ./scripts/clean-build-artifacts.sh"
    echo ""
    echo "   Or manually:"
    echo "     cargo clean"
    echo ""
    exit 1
fi

echo "✅ target/ size is reasonable (under ${THRESHOLD_GB}GB threshold)"
