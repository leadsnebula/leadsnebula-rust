#!/usr/bin/env bash
# Validate that SSM /leadsnebula/prod/rust/db/connection_url points to the prod Neon branch.
# Prevents prod API from using dev DB (which causes e.g. "create ping tree" 500 when instance_id
# exists only in prod).
set -euo pipefail

SSM_PATH="${SSM_PATH:-/leadsnebula/prod/rust/db/connection_url}"
PROD_HOST="${PROD_HOST:-ep-fragrant-bar}"
DEV_HOST="${DEV_HOST:-ep-bitter-frog}"

if ! command -v aws &>/dev/null; then
  echo "⚠️  aws CLI not found - skipping prod DB URL validation"
  exit 0
fi

VAL=$(aws ssm get-parameter --name "$SSM_PATH" --with-decryption --query 'Parameter.Value' --output text 2>/dev/null || true)
if [ -z "${VAL:-}" ]; then
  echo "⚠️  Could not read $SSM_PATH (missing AWS access or parameter) - skipping validation"
  exit 0
fi

if echo "$VAL" | grep -qF "$DEV_HOST"; then
  echo "❌ Production DB URL in SSM points to DEV branch ($DEV_HOST)"
  echo "   SSM path: $SSM_PATH"
  echo "   Fix: set this parameter to the prod Neon branch URL (host must contain $PROD_HOST)"
  echo "   Example: postgresql://...@${PROD_HOST}-...pooler....neon.tech/neondb?sslmode=require"
  exit 1
fi

if ! echo "$VAL" | grep -qF "$PROD_HOST"; then
  echo "❌ Production DB URL in SSM does not contain expected prod host ($PROD_HOST)"
  echo "   SSM path: $SSM_PATH"
  echo "   URL (masked): ${VAL:0:50}..."
  exit 1
fi

echo "✅ Prod DB URL in SSM points to prod branch ($PROD_HOST)"
