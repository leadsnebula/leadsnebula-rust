#!/usr/bin/env bash
# Validate and refresh database connection strings
# Fetches direct (non-pooled) connection strings from Neon and stores in SSM
# Supports both dev and prod environments

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Defaults
ENVIRONMENT=""
AWS_REGION="${AWS_REGION:-us-east-1}"

# Parse arguments
usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Validate and refresh database connection strings in SSM

OPTIONS:
    --env ENV          Environment (dev|prod|development|production) [required]
    --region REGION    AWS region [default: us-east-1]
    -h, --help         Show this help message

EXAMPLES:
    # Validate/refresh dev connection strings
    $0 --env dev

    # Validate/refresh prod connection strings
    $0 --env prod

NOTES:
    - Fetches direct (non-pooled) connection strings from Neon
    - Stores in SSM at /leadsnebula/{env}/rust/db/connection_url_direct
    - Requires Neon CLI and AWS CLI to be configured
EOF
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --env)
            ENVIRONMENT="$2"
            shift 2
            ;;
        --region)
            AWS_REGION="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            usage
            ;;
    esac
done

# Normalize environment name
normalize_env() {
    case "$1" in
        production) echo "prod" ;;
        development) echo "dev" ;;
        *) echo "$1" ;;
    esac
}

if [ -z "$ENVIRONMENT" ]; then
    echo -e "${RED}Error: --env is required${NC}"
    usage
fi

ENV_NORMALIZED=$(normalize_env "$ENVIRONMENT")
SSM_PATH="/leadsnebula/${ENV_NORMALIZED}/rust/db/connection_url_direct"

echo -e "${BLUE}=== Validate & Refresh Database Connection Strings ===${NC}"
echo -e "${BLUE}Environment: $ENVIRONMENT (normalized: $ENV_NORMALIZED)${NC}"
echo -e "${BLUE}SSM Path: $SSM_PATH${NC}"
echo -e "${BLUE}AWS Region: $AWS_REGION${NC}"
echo ""

# Check prerequisites
if ! command -v neonctl &> /dev/null; then
    echo -e "${RED}❌ Error: neonctl not found${NC}"
    echo "Install Neon CLI: https://neon.tech/docs/reference/neon-cli"
    exit 1
fi

if ! command -v aws &> /dev/null; then
    echo -e "${RED}❌ Error: AWS CLI not found${NC}"
    echo "Install AWS CLI: https://aws.amazon.com/cli/"
    exit 1
fi

if [ -z "${AWS_ACCESS_KEY_ID:-}" ] || [ -z "${AWS_SECRET_ACCESS_KEY:-}" ]; then
    echo -e "${RED}❌ Error: AWS credentials not set${NC}"
    echo "Required: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY"
    exit 1
fi

echo -e "${GREEN}✅ Prerequisites check passed${NC}"
echo ""

# Check Neon authentication
if ! neonctl whoami &> /dev/null; then
    echo -e "${RED}❌ Error: Not authenticated with Neon${NC}"
    echo "Run: neonctl auth"
    exit 1
fi

echo -e "${GREEN}✅ Neon authentication verified${NC}"
echo ""

# Map environment to Neon branch name
case "$ENV_NORMALIZED" in
    dev)
        NEON_BRANCH="development"
        ;;
    prod)
        NEON_BRANCH="production"
        ;;
    *)
        echo -e "${RED}❌ Error: Unknown environment: $ENV_NORMALIZED${NC}"
        echo "Supported: dev, prod, development, production"
        exit 1
        ;;
esac

echo -e "${BLUE}Fetching direct connection string from Neon (branch: $NEON_BRANCH)...${NC}"

# Try to get direct (non-pooled) connection string
CONN_STRING=$(neonctl connection-string "$NEON_BRANCH" --pooled=false 2>/dev/null || \
              neonctl connection-string "$NEON_BRANCH" 2>/dev/null || \
              echo "")

if [ -z "$CONN_STRING" ]; then
    echo -e "${RED}❌ Error: Failed to get connection string from Neon${NC}"
    echo "Make sure branch '$NEON_BRANCH' exists"
    exit 1
fi

# Mask password for display (show only first 5 chars)
MASKED_CONN=$(echo "$CONN_STRING" | sed 's/:[^@]*@/:***@/')
echo -e "${GREEN}✅ Connection string retrieved${NC}"
echo "   $MASKED_CONN"
echo ""

# Validate connection string format
if echo "$CONN_STRING" | grep -q "pooler"; then
    echo -e "${YELLOW}⚠️  WARNING: Connection string appears to be pooled${NC}"
    echo -e "${YELLOW}   For pg_dump/pg_restore, direct (non-pooled) strings are required${NC}"
    echo -e "${YELLOW}   Go to Neon Console → Connect → UNCHECK 'Connection pooling'${NC}"
    echo ""
    read -p "Continue anyway? (y/N): " confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        echo "Aborted."
        exit 1
    fi
else
    echo -e "${GREEN}✅ Connection string is direct (non-pooled)${NC}"
fi

echo ""

# Test connection
echo -e "${BLUE}Testing connection...${NC}"

# Check if psql is available
if command -v psql &> /dev/null; then
    if psql "$CONN_STRING" -c "SELECT 1;" &>/dev/null; then
        echo -e "${GREEN}✅ Connection test successful${NC}"
    else
        echo -e "${YELLOW}⚠️  Connection test failed (psql test)${NC}"
        echo "   This may be normal if credentials need updating"
    fi
elif command -v pg_isready &> /dev/null; then
    # Extract host and port from connection string
    HOST=$(echo "$CONN_STRING" | sed -n 's/.*@\([^:]*\):.*/\1/p')
    PORT=$(echo "$CONN_STRING" | sed -n 's/.*:\([0-9]*\)\/.*/\1/p' || echo "5432")
    
    if pg_isready -h "$HOST" -p "${PORT:-5432}" &>/dev/null; then
        echo -e "${GREEN}✅ Connection test successful (pg_isready)${NC}"
    else
        echo -e "${YELLOW}⚠️  Connection test failed (pg_isready)${NC}"
    fi
else
    echo -e "${YELLOW}⚠️  Skipping connection test (psql/pg_isready not available)${NC}"
fi

echo ""

# Store in SSM
echo -e "${BLUE}Storing connection string in SSM...${NC}"
echo "   Path: $SSM_PATH"
echo "   Type: SecureString"

# Check if parameter already exists
EXISTING=$(aws ssm get-parameter \
    --name "$SSM_PATH" \
    --region "$AWS_REGION" \
    --query 'Parameter.Value' \
    --output text 2>/dev/null || echo "")

if [ -n "$EXISTING" ]; then
    MASKED_EXISTING=$(echo "$EXISTING" | sed 's/:[^@]*@/:***@/')
    echo -e "${YELLOW}⚠️  Parameter already exists:${NC}"
    echo "   $MASKED_EXISTING"
    echo ""
    
    if [ "$EXISTING" = "$CONN_STRING" ]; then
        echo -e "${GREEN}✅ Connection string matches existing value${NC}"
        echo "   No update needed"
        exit 0
    else
        echo -e "${YELLOW}Connection string differs from existing value${NC}"
        read -p "Overwrite? (y/N): " confirm
        if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
            echo "Aborted."
            exit 0
        fi
    fi
fi

# Store in SSM
if aws ssm put-parameter \
    --name "$SSM_PATH" \
    --value "$CONN_STRING" \
    --type "SecureString" \
    --description "Direct (non-pooled) database connection URL for $ENVIRONMENT environment - used by pg_dump/pg_restore" \
    --region "$AWS_REGION" \
    --overwrite &>/dev/null; then
    echo -e "${GREEN}✅ Connection string stored in SSM${NC}"
else
    echo -e "${RED}❌ Error: Failed to store connection string in SSM${NC}"
    echo "   Check AWS credentials and IAM permissions"
    echo "   Required permission: ssm:PutParameter"
    exit 1
fi

echo ""

# Verify stored value
echo -e "${BLUE}Verifying stored value...${NC}"
VERIFIED=$(aws ssm get-parameter \
    --name "$SSM_PATH" \
    --region "$AWS_REGION" \
    --with-decryption \
    --query 'Parameter.Value' \
    --output text 2>/dev/null || echo "")

if [ "$VERIFIED" = "$CONN_STRING" ]; then
    echo -e "${GREEN}✅ Verification successful${NC}"
else
    echo -e "${RED}❌ Error: Stored value does not match${NC}"
    exit 1
fi

echo ""
echo -e "${GREEN}=== Complete ===${NC}"
echo ""
echo "Connection string stored at: $SSM_PATH"
echo "Next steps:"
echo "1. Validate SSM config: rust/scripts/validate-ssm-config.sh $ENVIRONMENT"
echo "2. Run database sync dry-run: helperScripts/sync-db.sh --mode=schema --dry-run"
