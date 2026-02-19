#!/usr/bin/env bash
# Run leads diagnostic SQL against DATABASE_URL (from .env.local or env).
# Usage: from rust/  ./scripts/run-diagnose-leads.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$RUST_ROOT"

if [ -f ".env.local" ]; then
    export $(grep -v '^#' .env.local | grep -v '^$' | xargs)
fi

if [ -z "${DATABASE_URL:-}" ]; then
    echo "DATABASE_URL is not set. Set it in .env.local or export it." >&2
    exit 1
fi

psql "$DATABASE_URL" -f "$SCRIPT_DIR/diagnose-leads-not-showing.sql"
