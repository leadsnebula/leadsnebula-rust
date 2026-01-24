#!/bin/bash
# scripts/review-deletions.sh
# Review deletions before committing
#
# This script shows what files are being deleted and allows you to review them
# before committing. It's a safety measure to prevent accidental deletions.
#
# Usage:
#   ./scripts/review-deletions.sh              # Review staged deletions
#   ./scripts/review-deletions.sh --all        # Review all deletions (staged + unstaged)
#   ./scripts/review-deletions.sh --commit     # Review deletions in a specific commit

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

MODE="${1:-staged}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔍 Reviewing file deletions"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

case "$MODE" in
    --all)
        echo "Reviewing ALL deletions (staged + unstaged)..."
        DELETED_FILES=$(git diff --name-only --diff-filter=D HEAD 2>/dev/null || echo "")
        ;;
    --commit)
        if [ -z "${2:-}" ]; then
            echo "❌ ERROR: --commit requires a commit hash" >&2
            echo "Usage: $0 --commit <commit-hash>" >&2
            exit 1
        fi
        COMMIT="${2}"
        echo "Reviewing deletions in commit: $COMMIT"
        DELETED_FILES=$(git show --name-status --diff-filter=D --format="" "$COMMIT" 2>/dev/null | grep "^D" | cut -c3- || echo "")
        ;;
    *)
        echo "Reviewing STAGED deletions..."
        DELETED_FILES=$(git diff --cached --name-only --diff-filter=D 2>/dev/null || echo "")
        ;;
esac

if [ -z "$DELETED_FILES" ]; then
    echo "✅ No deletions found"
    exit 0
fi

# Count deletions
DELETED_COUNT=$(echo "$DELETED_FILES" | grep -v '^$' | wc -l || echo "0")
echo "Found $DELETED_COUNT deleted file(s):"
echo ""

# Show deleted files grouped by type
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📋 Deleted Files:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Group by extension
RS_FILES=()
TOML_FILES=()
SQL_FILES=()
SH_FILES=()
YML_FILES=()
OTHER_FILES=()

while IFS= read -r file; do
    if [ -z "$file" ]; then
        continue
    fi
    
    case "$file" in
        *.rs) RS_FILES+=("$file") ;;
        *.toml) TOML_FILES+=("$file") ;;
        *.sql) SQL_FILES+=("$file") ;;
        *.sh) SH_FILES+=("$file") ;;
        *.yml|*.yaml) YML_FILES+=("$file") ;;
        *) OTHER_FILES+=("$file") ;;
    esac
done <<< "$DELETED_FILES"

# Show by category
if [ ${#RS_FILES[@]} -gt 0 ]; then
    echo ""
    echo "⚠️  Rust source files (*.rs):"
    for file in "${RS_FILES[@]}"; do
        echo "  - $file"
    done
fi

if [ ${#TOML_FILES[@]} -gt 0 ]; then
    echo ""
    echo "⚠️  Cargo files (*.toml):"
    for file in "${TOML_FILES[@]}"; do
        echo "  - $file"
    done
fi

if [ ${#SQL_FILES[@]} -gt 0 ]; then
    echo ""
    echo "⚠️  SQL migrations (*.sql):"
    for file in "${SQL_FILES[@]}"; do
        echo "  - $file"
    done
fi

if [ ${#SH_FILES[@]} -gt 0 ]; then
    echo ""
    echo "⚠️  Shell scripts (*.sh):"
    for file in "${SH_FILES[@]}"; do
        echo "  - $file"
    done
fi

if [ ${#YML_FILES[@]} -gt 0 ]; then
    echo ""
    echo "⚠️  YAML files (*.yml, *.yaml):"
    for file in "${YML_FILES[@]}"; do
        echo "  - $file"
    done
fi

if [ ${#OTHER_FILES[@]} -gt 0 ]; then
    echo ""
    echo "Other files:"
    for file in "${OTHER_FILES[@]}"; do
        echo "  - $file"
    done
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Check for protected files
PROTECTED_COUNT=$((${#RS_FILES[@]} + ${#TOML_FILES[@]} + ${#SQL_FILES[@]} + ${#SH_FILES[@]} + ${#YML_FILES[@]}))

if [ $PROTECTED_COUNT -gt 0 ]; then
    echo ""
    echo "❌ WARNING: Protected source files are being deleted!"
    echo ""
    echo "Protected file types (should never be deleted):"
    echo "  - *.rs (Rust source files)"
    echo "  - *.toml (Cargo configuration)"
    echo "  - *.sql (Database migrations)"
    echo "  - *.sh (Shell scripts)"
    echo "  - *.yml, *.yaml (Configuration files)"
    echo ""
    echo "If these deletions are intentional, verify:"
    echo "  1. Files are truly obsolete (not just moved/renamed)"
    echo "  2. No other code depends on these files"
    echo "  3. These are not critical application files"
    echo ""
    exit 1
fi

echo ""
echo "✅ No protected source files being deleted"
echo ""
echo "To see the actual diff of deletions:"
if [ "$MODE" = "--commit" ]; then
    echo "  git show $COMMIT --stat"
    echo "  git show $COMMIT --diff-filter=D"
else
    echo "  git diff --cached --diff-filter=D"
fi
