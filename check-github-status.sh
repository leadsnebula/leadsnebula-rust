#!/bin/bash
# Quick script to check GitHub Actions status

REPO="leadsnebula/leadsnebula-rust"
BRANCH="dev"

echo "🔍 Checking GitHub Actions status for $REPO..."
echo ""

# Check if workflows exist
echo "📋 Available Workflows:"
curl -s "https://api.github.com/repos/$REPO/actions/workflows" | \
  grep -o '"name":"[^"]*"' | \
  sed 's/"name":"//g' | \
  sed 's/"//g' | \
  sort | \
  while read workflow; do
    echo "  ✅ $workflow"
  done

echo ""
echo "📊 Recent Workflow Runs:"
echo "Visit: https://github.com/$REPO/actions"
echo ""
echo "🔗 Direct Links:"
echo "  - All workflows: https://github.com/$REPO/actions"
echo "  - CI workflow: https://github.com/$REPO/actions/workflows/ci.yml"
echo "  - Deploy Dev: https://github.com/$REPO/actions/workflows/deploy-dev.yml"
echo "  - Deploy Staging: https://github.com/$REPO/actions/workflows/deploy-staging.yml"
echo "  - Deploy Production: https://github.com/$REPO/actions/workflows/deploy-production.yml"
echo ""
echo "💡 To see detailed status, visit the links above or use:"
echo "   gh workflow list  (if GitHub CLI is installed)"
echo "   gh run list       (if GitHub CLI is installed)"

