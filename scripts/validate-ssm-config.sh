
#!/bin/bash
# Validate SSM Parameter Store configuration before deployment
# This script checks that all required SSM parameters exist for the given environment

set -euo pipefail

ENVIRONMENT="${1:-}"
if [ -z "$ENVIRONMENT" ]; then
    echo "Usage: $0 <environment>"
    echo "Example: $0 production"
    exit 1
fi

# Normalize environment name (production -> prod, development -> dev)
normalize_env() {
    case "$1" in
        "production") echo "prod" ;;
        "development") echo "dev" ;;
        *) echo "$1" ;;
    esac
}

ENV_NORMALIZED=$(normalize_env "$ENVIRONMENT")

echo "🔍 Validating SSM configuration for environment: $ENVIRONMENT (normalized: $ENV_NORMALIZED)"
echo ""

# Required SSM paths
REQUIRED_PATHS=(
    "/leadsnebula/${ENV_NORMALIZED}/rust/db/connection_url"
    "/leadsnebula/${ENV_NORMALIZED}/rust/db/connection_url_direct"  # Direct connection for migrations
    "/leadsnebula/${ENV_NORMALIZED}/rust/auth/jwt_secret"
    "/leadsnebula/${ENV_NORMALIZED}/rust/encryption/api_key_key"
    "/leadsnebula/prod/rust/redis/connection_url"  # Redis is always from prod path
)

# Optional paths (warnings only)
OPTIONAL_PATHS=(
    "/leadsnebula/${ENV_NORMALIZED}/rust/monitoring/sentry_dsn"
    "/leadsnebula/${ENV_NORMALIZED}/rust/email/from_address"
)

# Check AWS credentials
if [ -z "${AWS_ACCESS_KEY_ID:-}" ] || [ -z "${AWS_SECRET_ACCESS_KEY:-}" ]; then
    echo "❌ ERROR: AWS credentials not set"
    echo "   Required: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION"
    exit 1
fi

if [ -z "${AWS_REGION:-}" ]; then
    echo "❌ ERROR: AWS_REGION not set"
    exit 1
fi

echo "✅ AWS credentials configured"
echo ""

# Check if AWS CLI is available
if ! command -v aws &> /dev/null; then
    echo "⚠️  WARNING: AWS CLI not found. Installing..."
    # Try to install AWS CLI v2 (simplified - may need adjustment)
    echo "   Please install AWS CLI: https://aws.amazon.com/cli/"
    exit 1
fi

# Validate required paths
MISSING_PATHS=()
for path in "${REQUIRED_PATHS[@]}"; do
    if aws ssm get-parameter --name "$path" --region "${AWS_REGION}" --query 'Parameter.Value' --output text &>/dev/null; then
        echo "✅ Found: $path"
    else
        echo "❌ Missing: $path"
        MISSING_PATHS+=("$path")
    fi
done

# Check optional paths
for path in "${OPTIONAL_PATHS[@]}"; do
    if aws ssm get-parameter --name "$path" --region "${AWS_REGION}" --query 'Parameter.Value' --output text &>/dev/null; then
        echo "✅ Found (optional): $path"
    else
        echo "⚠️  Missing (optional): $path"
    fi
done

echo ""

if [ ${#MISSING_PATHS[@]} -gt 0 ]; then
    echo "❌ ERROR: Missing required SSM parameters:"
    for path in "${MISSING_PATHS[@]}"; do
        echo "   - $path"
    done
    echo ""
    echo "Create them with:"
    echo "  aws ssm put-parameter --name \"<path>\" --value \"<value>\" --type \"SecureString\" --region ${AWS_REGION}"
    exit 1
fi

echo "✅ All required SSM parameters are present"
exit 0
