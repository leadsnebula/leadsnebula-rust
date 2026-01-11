#!/bin/bash
# SSM Parameter Store diagnostics and validation
# Combines validation and diagnostic capabilities for SSM configuration
#
# Usage:
#   ./ssm-diagnostics.sh validate <environment>  # Validate SSM configuration
#   ./ssm-diagnostics.sh diagnose [environment] # Diagnose SSM access issues

set -euo pipefail

MODE="${1:-}"
ENVIRONMENT="${2:-}"

# Normalize environment name
normalize_env() {
    case "$1" in
        "production") echo "prod" ;;
        "development") echo "dev" ;;
        *) echo "$1" ;;
    esac
}

# Validate SSM configuration
validate() {
    if [ -z "$ENVIRONMENT" ]; then
        echo "Usage: $0 validate <environment>"
        echo "Example: $0 validate production"
        exit 1
    fi

    ENV_NORMALIZED=$(normalize_env "$ENVIRONMENT")

    echo "🔍 Validating SSM configuration for environment: $ENVIRONMENT (normalized: $ENV_NORMALIZED)"
    echo ""

    # Required SSM paths
    REQUIRED_PATHS=(
        "/leadsnebula/${ENV_NORMALIZED}/rust/db/connection_url"
        "/leadsnebula/${ENV_NORMALIZED}/rust/db/connection_url_direct"
        "/leadsnebula/${ENV_NORMALIZED}/rust/auth/jwt_secret"
        "/leadsnebula/${ENV_NORMALIZED}/rust/encryption/api_key_key"
        "/leadsnebula/prod/rust/redis/connection_url"
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
        echo "❌ ERROR: AWS CLI not found"
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
}

# Diagnose SSM access issues
diagnose() {
    ENV_NORMALIZED=$(normalize_env "${ENVIRONMENT:-production}")

    echo "🔍 SSM Access Diagnostic"
    echo "========================"
    echo ""

    # Check environment variables
    echo "1. Checking environment variables:"
    if [ -z "${ENVIRONMENT:-}" ]; then
        echo "   ❌ ENVIRONMENT not set"
    else
        echo "   ✅ ENVIRONMENT=$ENVIRONMENT"
    fi

    if [ -z "${AWS_ACCESS_KEY_ID:-}" ]; then
        echo "   ❌ AWS_ACCESS_KEY_ID not set"
    else
        echo "   ✅ AWS_ACCESS_KEY_ID is set (length: ${#AWS_ACCESS_KEY_ID})"
    fi

    if [ -z "${AWS_SECRET_ACCESS_KEY:-}" ]; then
        echo "   ❌ AWS_SECRET_ACCESS_KEY not set"
    else
        echo "   ✅ AWS_SECRET_ACCESS_KEY is set (length: ${#AWS_SECRET_ACCESS_KEY})"
    fi

    if [ -z "${AWS_REGION:-}" ]; then
        echo "   ⚠️  AWS_REGION not set (will use default)"
    else
        echo "   ✅ AWS_REGION=$AWS_REGION"
    fi

    echo ""

    echo "2. Environment normalization:"
    echo "   ENVIRONMENT: ${ENVIRONMENT:-production}"
    echo "   Normalized: $ENV_NORMALIZED"
    echo "   Expected SSM path: /leadsnebula/$ENV_NORMALIZED/rust/"
    echo ""

    # Test SSM access (if AWS CLI is available)
    if command -v aws &> /dev/null; then
        echo "3. Testing SSM access with AWS CLI:"
        TEST_PATH="/leadsnebula/$ENV_NORMALIZED/rust/db/connection_url"
        echo "   Testing path: $TEST_PATH"

        if aws ssm get-parameter --name "$TEST_PATH" --with-decryption --region "${AWS_REGION:-us-east-1}" &>/dev/null; then
            echo "   ✅ Successfully accessed SSM parameter"
        else
            echo "   ❌ Failed to access SSM parameter"
            echo "   Error details:"
            aws ssm get-parameter --name "$TEST_PATH" --with-decryption --region "${AWS_REGION:-us-east-1}" 2>&1 || true
        fi
    else
        echo "3. AWS CLI not available (this is normal in production)"
        echo "   SSM access will be tested by the application at startup"
    fi

    echo ""
    echo "4. Expected SSM parameters for $ENV_NORMALIZED:"
    echo "   - /leadsnebula/$ENV_NORMALIZED/rust/db/connection_url"
    echo "   - /leadsnebula/$ENV_NORMALIZED/rust/auth/jwt_secret"
    echo "   - /leadsnebula/$ENV_NORMALIZED/rust/encryption/api_key_key"
    echo "   - /leadsnebula/prod/rust/redis/connection_url (always prod path)"
    echo ""

    echo "5. Troubleshooting:"
    echo "   If SSM access fails, check:"
    echo "   - AWS credentials have IAM policy attached (see iam-policy.json)"
    echo "   - AWS_REGION matches the region where SSM parameters are stored"
    echo "   - IAM policy allows access to: arn:aws:ssm:*:*:parameter/leadsnebula/*"
    echo "   - KMS permissions for decrypting SecureString parameters"
    echo ""
}

# Main command dispatcher
case "$MODE" in
    validate)
        validate
        ;;
    diagnose)
        diagnose
        ;;
    *)
        echo "Usage: $0 {validate|diagnose} [environment]"
        echo ""
        echo "Commands:"
        echo "  validate <environment>  Validate SSM configuration for an environment"
        echo "  diagnose [environment]   Diagnose SSM access issues (defaults to production)"
        echo ""
        echo "Examples:"
        echo "  $0 validate production"
        echo "  $0 diagnose dev"
        exit 1
        ;;
esac
