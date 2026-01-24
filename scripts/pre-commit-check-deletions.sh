#!/bin/bash
# scripts/pre-commit-check-deletions.sh
# Pre-commit hook to prevent accidental deletion of source files
#
# This script checks for deletions of critical file types and warns/errors
# Never delete: *.rs, *.toml, *.sql, *.sh, *.yml, *.yaml
#
# Usage:
#   ./scripts/pre-commit-check-deletions.sh
#   Or install as git pre-commit hook:
#   ln -s ../../scripts/pre-commit-check-deletions.sh .git/hooks/pre-commit

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# Protected file patterns (never delete these)
PROTECTED_PATTERNS=(
    "*.rs"
    "*.toml"
    "*.sql"
    "*.sh"
    "*.yml"
    "*.yaml"
    "*.yaml"
)

# Get list of deleted files in staging area
DELETED_FILES=$(git diff --cached --name-only --diff-filter=D 2>/dev/null || echo "")

if [ -z "$DELETED_FILES" ]; then
    # No deletions, all good
    exit 0
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔍 Checking for protected file deletions..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

VIOLATIONS=0
VIOLATION_FILES=()

# Check each deleted file against protected patterns
while IFS= read -r file; do
    if [ -z "$file" ]; then
        continue
    fi
    
    # Check if file matches any protected pattern
    for pattern in "${PROTECTED_PATTERNS[@]}"; do
        if [[ "$file" == $pattern ]] || [[ "$file" == *"/$pattern" ]]; then
            VIOLATIONS=$((VIOLATIONS + 1))
            VIOLATION_FILES+=("$file")
            break
        fi
    done
done <<< "$DELETED_FILES"

# Report violations
if [ $VIOLATIONS -gt 0 ]; then
    echo "❌ ERROR: Attempting to delete protected source files!"
    echo ""
    echo "The following protected files are being deleted:"
    for file in "${VIOLATION_FILES[@]}"; do
        echo "  - $file"
    done
    echo ""
    echo "Protected file types (never delete):"
    for pattern in "${PROTECTED_PATTERNS[@]}"; do
        echo "  - $pattern"
    done
    echo ""
    echo "If you really need to delete these files:"
    echo "  1. Review the deletion carefully"
    echo "  2. Use: git commit --no-verify (bypasses this check)"
    echo ""
    echo "To unstage these deletions:"
    echo "  git restore --staged ${VIOLATION_FILES[*]}"
    echo ""
    exit 1
fi

# Show summary of deletions (for awareness)
DELETED_COUNT=$(echo "$DELETED_FILES" | grep -v '^$' | wc -l || echo "0")
if [ "$DELETED_COUNT" -gt 0 ]; then
    echo "⚠️  Warning: $DELETED_COUNT file(s) are being deleted:"
    echo "$DELETED_FILES" | while IFS= read -r file; do
        if [ -n "$file" ]; then
            echo "  - $file"
        fi
    done
    echo ""
    echo "Review these deletions carefully before committing."
    echo "To see what's being deleted:"
    echo "  git diff --cached --diff-filter=D"
    echo ""
    read -p "Continue with commit? (yes/no): " confirm
    if [ "$confirm" != "yes" ]; then
        echo "❌ Commit aborted"
        exit 1
    fi
fi

echo "✅ No protected files being deleted"
exit 0
