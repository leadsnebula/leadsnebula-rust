#!/usr/bin/env bash
# Verify Neon branches and get connection strings
# Checks for production, development, and defaults branches
# Prepares for defaults branch deletion with enhanced verification

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Neon Branch Verification ===${NC}"
echo ""

# Check if Neon CLI is installed
if ! command -v neonctl &> /dev/null; then
    echo -e "${RED}❌ Error: neonctl not found${NC}"
    echo "Install Neon CLI: https://neon.tech/docs/reference/neon-cli"
    exit 1
fi

echo -e "${GREEN}✅ Neon CLI found${NC}"
echo ""

# Check if authenticated
if ! neonctl whoami &> /dev/null; then
    echo -e "${RED}❌ Error: Not authenticated with Neon${NC}"
    echo "Run: neonctl auth"
    exit 1
fi

echo -e "${GREEN}✅ Authenticated with Neon${NC}"
echo ""

# List all branches
echo -e "${BLUE}Listing all branches...${NC}"
BRANCHES=$(neonctl branches list --output json 2>/dev/null || echo "[]")

if [ "$BRANCHES" = "[]" ] || [ -z "$BRANCHES" ]; then
    echo -e "${RED}❌ Error: No branches found or failed to list branches${NC}"
    exit 1
fi

echo "$BRANCHES" | jq -r '.[] | "\(.name) (ID: \(.id))"' || {
    echo -e "${YELLOW}⚠️  Warning: Could not parse branch list (jq not available?)${NC}"
    echo "Raw output:"
    echo "$BRANCHES"
}

echo ""

# Check for required branches
REQUIRED_BRANCHES=("production" "development")
MISSING_BRANCHES=()

for branch in "${REQUIRED_BRANCHES[@]}"; do
    if echo "$BRANCHES" | jq -e ".[] | select(.name == \"$branch\")" &>/dev/null; then
        echo -e "${GREEN}✅ Found branch: $branch${NC}"
    else
        echo -e "${RED}❌ Missing branch: $branch${NC}"
        MISSING_BRANCHES+=("$branch")
    fi
done

# Check for defaults branch
if echo "$BRANCHES" | jq -e ".[] | select(.name == \"defaults\")" &>/dev/null; then
    echo -e "${YELLOW}⚠️  Found branch: defaults${NC}"
    HAS_DEFAULTS=true
else
    echo -e "${GREEN}✅ No defaults branch found (already cleaned up)${NC}"
    HAS_DEFAULTS=false
fi

echo ""

if [ ${#MISSING_BRANCHES[@]} -gt 0 ]; then
    echo -e "${RED}❌ ERROR: Missing required branches:${NC}"
    for branch in "${MISSING_BRANCHES[@]}"; do
        echo "   - $branch"
    done
    exit 1
fi

# Get direct connection strings for required branches
echo -e "${BLUE}=== Getting Direct Connection Strings ===${NC}"
echo ""

for branch in "${REQUIRED_BRANCHES[@]}"; do
    echo -e "${BLUE}Fetching connection string for: $branch${NC}"
    
    # Try to get direct (non-pooled) connection string
    CONN_STRING=$(neonctl connection-string "$branch" --pooled=false 2>/dev/null || \
                  neonctl connection-string "$branch" 2>/dev/null || \
                  echo "")
    
    if [ -z "$CONN_STRING" ]; then
        echo -e "${RED}❌ Failed to get connection string for $branch${NC}"
        continue
    fi
    
    # Mask password in output (show only first 5 chars)
    MASKED_CONN=$(echo "$CONN_STRING" | sed 's/:[^@]*@/:***@/')
    
    # Verify it's not pooled
    if echo "$CONN_STRING" | grep -q "pooler"; then
        echo -e "${YELLOW}⚠️  WARNING: Connection string appears to be pooled${NC}"
        echo "   $MASKED_CONN"
        echo -e "${YELLOW}   Use Neon Console → Connect → UNCHECK 'Connection pooling'${NC}"
    else
        echo -e "${GREEN}✅ Direct connection string (non-pooled)${NC}"
        echo "   $MASKED_CONN"
    fi
    echo ""
done

# Enhanced verification for defaults branch before deletion
if [ "$HAS_DEFAULTS" = true ]; then
    echo -e "${YELLOW}=== Defaults Branch Verification (Before Deletion) ===${NC}"
    echo ""
    echo -e "${YELLOW}⚠️  WARNING: Defaults branch is usually empty — but confirm size/age first${NC}"
    echo ""
    
    # Get detailed information about defaults branch
    DEFAULTS_INFO=$(neonctl branches get defaults --output json 2>/dev/null || echo "{}")
    
    if [ "$DEFAULTS_INFO" != "{}" ] && [ -n "$DEFAULTS_INFO" ]; then
        echo -e "${BLUE}Defaults Branch Details:${NC}"
        
        # Extract and display key information
        BRANCH_ID=$(echo "$DEFAULTS_INFO" | jq -r '.id // "unknown"' 2>/dev/null || echo "unknown")
        CREATED_AT=$(echo "$DEFAULTS_INFO" | jq -r '.created_at // "unknown"' 2>/dev/null || echo "unknown")
        UPDATED_AT=$(echo "$DEFAULTS_INFO" | jq -r '.updated_at // "unknown"' 2>/dev/null || echo "unknown")
        
        echo "   Branch ID: $BRANCH_ID"
        echo "   Created: $CREATED_AT"
        echo "   Last Updated: $UPDATED_AT"
        
        # Check for compute resources
        COMPUTE_COUNT=$(echo "$DEFAULTS_INFO" | jq -r '.compute | length // 0' 2>/dev/null || echo "0")
        if [ "$COMPUTE_COUNT" != "0" ] && [ "$COMPUTE_COUNT" != "null" ]; then
            echo -e "${YELLOW}   ⚠️  Compute resources found: $COMPUTE_COUNT${NC}"
            echo "$DEFAULTS_INFO" | jq -r '.compute[]? | "      - \(.name // .id) (\(.type // "unknown"))"' 2>/dev/null || true
        else
            echo -e "${GREEN}   ✅ No compute resources (safe to delete)${NC}"
        fi
        
        # Check for endpoints
        ENDPOINT_COUNT=$(echo "$DEFAULTS_INFO" | jq -r '.endpoints | length // 0' 2>/dev/null || echo "0")
        if [ "$ENDPOINT_COUNT" != "0" ] && [ "$ENDPOINT_COUNT" != "null" ]; then
            echo -e "${YELLOW}   ⚠️  Endpoints found: $ENDPOINT_COUNT${NC}"
        else
            echo -e "${GREEN}   ✅ No endpoints${NC}"
        fi
        
        echo ""
        echo -e "${BLUE}Full branch info (JSON):${NC}"
        echo "$DEFAULTS_INFO" | jq '.' 2>/dev/null || echo "$DEFAULTS_INFO"
    else
        echo -e "${YELLOW}⚠️  Could not retrieve detailed defaults branch information${NC}"
        echo "   You may need to check manually in Neon Console"
    fi
    
    echo ""
    echo -e "${YELLOW}To delete defaults branch, run:${NC}"
    echo "   rust/scripts/cleanup-neon-defaults-branch.sh"
    echo ""
fi

echo -e "${GREEN}=== Verification Complete ===${NC}"
echo ""
echo "Next steps:"
echo "1. Store direct connection strings in SSM: rust/scripts/validate-refresh-db-connections.sh"
echo "2. Run database sync dry-run: helperScripts/sync-db.sh --mode=schema --dry-run"
if [ "$HAS_DEFAULTS" = true ]; then
    echo "3. After successful sync, delete defaults branch: rust/scripts/cleanup-neon-defaults-branch.sh"
fi
