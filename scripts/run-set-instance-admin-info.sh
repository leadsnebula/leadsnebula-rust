#!/usr/bin/env bash
# Set Leads Test Instance owner to info@leadsnebula.com (instance admin).
# Usage: from rust/  ./scripts/run-set-instance-admin-info.sh

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

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$SCRIPT_DIR/set-instance-admin-info.sql"
