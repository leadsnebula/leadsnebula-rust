#!/bin/bash
# scripts/clean-build-artifacts.sh
# SAFE cleanup - only removes build artifacts, never source files
#
# This script safely cleans Rust build artifacts from the target/ directory.
# It will NEVER delete source files (*.rs, *.toml, *.sql, *.sh, *.yml, *.yaml)
#
# Usage:
#   ./scripts/clean-build-artifacts.sh          # Clean all build artifacts
#   ./scripts/clean-build-artifacts.sh --release  # Clean only release builds
#   ./scripts/clean-build-artifacts.sh --aggressive  # Also clean cargo registry cache

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

MODE="${1:-normal}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧹 Cleaning Rust build artifacts"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Safety check: Ensure we're in a Rust project
if [ ! -f "Cargo.toml" ]; then
    echo "❌ ERROR: Not in a Rust project (Cargo.toml not found)" >&2
    echo "   Run this script from the rust/ directory" >&2
    exit 1
fi

# Show size before
if [ -d "target" ]; then
    SIZE_BEFORE=$(du -sh target/ 2>/dev/null | cut -f1 || echo "0")
    SIZE_BEFORE_MB=$(du -sm target/ 2>/dev/null | cut -f1 || echo "0")
    echo "Current target/ size: $SIZE_BEFORE ($SIZE_BEFORE_MB MB)"
    echo ""
else
    echo "target/ directory does not exist (nothing to clean)"
    exit 0
fi

# Clean based on mode
case "$MODE" in
    --release)
        echo "Cleaning release builds only (keeps debug builds for faster dev)..."
        cargo clean --release
        ;;
    --aggressive)
        echo "⚠️  AGGRESSIVE MODE: Cleaning all build artifacts AND cargo registry cache"
        echo "   This will require re-downloading dependencies on next build"
        echo ""
        read -p "Continue? (yes/no): " confirm
        if [ "$confirm" != "yes" ]; then
            echo "❌ Aborted"
            exit 1
        fi
        cargo clean
        # Clean cargo registry cache (WARNING: Will re-download on next build)
        if [ -d "$HOME/.cargo/registry/cache" ]; then
            echo "Cleaning cargo registry cache..."
            rm -rf "$HOME/.cargo/registry/cache"/*
            echo "✅ Cargo registry cache cleaned"
        fi
        ;;
    *)
        echo "Cleaning all build artifacts (keeps downloaded dependencies)..."
        cargo clean
        ;;
esac

# Show size after
if [ -d "target" ]; then
    SIZE_AFTER=$(du -sh target/ 2>/dev/null | cut -f1 || echo "0")
    SIZE_AFTER_MB=$(du -sm target/ 2>/dev/null | cut -f1 || echo "0")
    echo ""
    echo "After cleanup: $SIZE_AFTER ($SIZE_AFTER_MB MB)"
    
    if [ "$SIZE_BEFORE_MB" -gt 0 ] && [ "$SIZE_AFTER_MB" -gt 0 ]; then
        SAVED=$((SIZE_BEFORE_MB - SIZE_AFTER_MB))
        echo "Space freed: ~${SAVED} MB"
    fi
else
    echo ""
    echo "target/ directory removed"
fi

echo ""
echo "✅ Cleanup complete!"
echo ""
echo "Note: This only cleaned build artifacts. Source files are untouched."
echo "      Next 'cargo build' will rebuild everything (slower first time)."
