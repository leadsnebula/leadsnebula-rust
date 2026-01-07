#!/bin/bash
# Diagnostic script to help identify SSM access issues
# This can be run in Fly.io via SSH to test SSM access

set -euo pipefail

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

# Normalize environment
normalize_env() {
    case "$1" in
        "production") echo "prod" ;;
        "development") echo "dev" ;;
        *) echo "$1" ;;
    esac
}

ENV_NORMALIZED=$(normalize_env "${ENVIRONMENT:-production}")
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
echo "   - /leadsnebula/$ENV_NORMALIZED/rust/auth/jwt_secret (or /leadsnebula/$ENV_NORMALIZED/rust/jwt/secret_key)"
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
