#!/bin/bash
# Script to store encryption keys in AWS SSM Parameter Store
# Run this script to create the encryption keys for dev and prod environments

set -e

# Get AWS region from environment or use default
AWS_REGION=${AWS_REGION:-us-east-1}

echo "Storing encryption keys in AWS SSM Parameter Store (region: $AWS_REGION)"
echo ""

# Dev encryption key
echo "Storing dev encryption key..."
aws ssm put-parameter \
  --region "$AWS_REGION" \
  --name "/leadsnebula/dev/rust/encryption/api_key_key" \
  --value "lpwGapuOQsKHIkN5M5qZUHdQwjriLp5J68TWejHMIsI=" \
  --type "SecureString" \
  --overwrite

echo "✅ Dev encryption key stored"
echo ""

# Prod encryption key
echo "Storing prod encryption key..."
aws ssm put-parameter \
  --region "$AWS_REGION" \
  --name "/leadsnebula/prod/rust/encryption/api_key_key" \
  --value "L2+Y+bYwuN8+vam9M6aBHQwhWfqNE68BpPAv8ajBmiM=" \
  --type "SecureString" \
  --overwrite

echo "✅ Prod encryption key stored"
echo ""

# Verify keys were stored
echo "Verifying keys..."
DEV_KEY=$(aws ssm get-parameter \
  --region "$AWS_REGION" \
  --name "/leadsnebula/dev/rust/encryption/api_key_key" \
  --with-decryption \
  --query "Parameter.Value" \
  --output text 2>/dev/null || echo "")

PROD_KEY=$(aws ssm get-parameter \
  --region "$AWS_REGION" \
  --name "/leadsnebula/prod/rust/encryption/api_key_key" \
  --with-decryption \
  --query "Parameter.Value" \
  --output text 2>/dev/null || echo "")

if [ -n "$DEV_KEY" ] && [ -n "$PROD_KEY" ]; then
  echo "✅ Both keys verified successfully"
  echo ""
  echo "Dev key path: /leadsnebula/dev/rust/encryption/api_key_key"
  echo "Prod key path: /leadsnebula/prod/rust/encryption/api_key_key"
else
  echo "❌ Error: Failed to verify keys"
  exit 1
fi
