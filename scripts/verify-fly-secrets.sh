#!/bin/bash
# Verify that required secrets are set in Fly.io app

set -euo pipefail

APP_NAME="${1:-}"
if [ -z "$APP_NAME" ]; then
    echo "Usage: $0 <fly-app-name>"
    echo "Example: $0 leadsnebula-rust"
    exit 1
fi

echo "🔍 Checking secrets for Fly.io app: $APP_NAME"
echo ""

# Check if flyctl is available
if ! command -v flyctl &> /dev/null; then
    echo "❌ ERROR: flyctl not found. Install it from https://fly.io/docs/hands-on/install-flyctl/"
    exit 1
fi

# List secrets (flyctl doesn't show values, just names)
echo "📋 Current secrets in Fly.io app:"
flyctl secrets list --app "$APP_NAME" || {
    echo "❌ Failed to list secrets. Check that:"
    echo "   1. You're logged in: flyctl auth login"
    echo "   2. You have access to app: $APP_NAME"
    exit 1
}

echo ""
echo "✅ Required secrets for production:"
echo "   - ENVIRONMENT (should be 'production')"
echo "   - AWS_ACCESS_KEY_ID"
echo "   - AWS_SECRET_ACCESS_KEY"
echo "   - AWS_REGION (e.g., 'us-east-1')"
echo ""
echo "To set secrets:"
echo "  flyctl secrets set KEY=value --app $APP_NAME"
echo ""
echo "To check if a specific secret is set:"
echo "  flyctl secrets list --app $APP_NAME | grep KEY_NAME"
