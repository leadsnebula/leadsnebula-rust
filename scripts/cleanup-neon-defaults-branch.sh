#!/usr/bin/env bash
# Cleanup Neon defaults branch
# Verifies defaults branch details before deletion

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Neon Defaults Branch Cleanup ===${NC}"
echo ""

# Check if Neon CLI is installed
if ! command -v neonctl &> /dev/null; then
    echo -e "${RED}❌ Error: neonctl not found${NC}"
    echo "Install Neon CLI: https://neon.tech/docs/reference/neon-cli"
    exit 1
fi

# Check if authenticated
if ! neonctl whoami &> /dev/null; then
    echo -e "${RED}❌ Error: Not authenticated with Neon${NC}"
    echo "Run: neonctl auth"
    exit 1
fi

echo -e "${GREEN}✅ Neon CLI authenticated${NC}"
echo ""

# Verify required branches exist
echo -e "${BLUE}Verifying required branches exist...${NC}"
BRANCHES=$(neonctl branches list --output json 2>/dev/null || echo "[]")

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

if [ ${#MISSING_BRANCHES[@]} -gt 0 ]; then
    echo -e "${RED}❌ ERROR: Missing required branches:${NC}"
    for branch in "${MISSING_BRANCHES[@]}"; do
        echo "   - $branch"
    done
    echo ""
    echo "Cannot proceed with defaults branch deletion if required branches are missing."
    exit 1
fi

echo ""

# Check if defaults branch exists
if echo "$BRANCHES" | jq -e ".[] | select(.name == \"defaults\")" &>/dev/null; then
    echo -e "${YELLOW}⚠️  Found defaults branch${NC}"
    HAS_DEFAULTS=true
else
    echo -e "${GREEN}✅ No defaults branch found (already cleaned up)${NC}"
    exit 0
fi

echo ""

# Enhanced verification: Get detailed information about defaults branch
echo -e "${BLUE}=== Enhanced Defaults Branch Verification ===${NC}"
echo ""

DEFAULTS_INFO=$(neonctl branches get defaults --output json 2>/dev/null || echo "{}")

if [ "$DEFAULTS_INFO" = "{}" ] || [ -z "$DEFAULTS_INFO" ]; then
    echo -e "${RED}❌ Error: Could not retrieve defaults branch information${NC}"
    exit 1
fi

echo -e "${BLUE}Defaults Branch Details:${NC}"
echo ""

# Extract and display key information
BRANCH_ID=$(echo "$DEFAULTS_INFO" | jq -r '.id // "unknown"' 2>/dev/null || echo "unknown")
CREATED_AT=$(echo "$DEFAULTS_INFO" | jq -r '.created_at // "unknown"' 2>/dev/null || echo "unknown")
UPDATED_AT=$(echo "$DEFAULTS_INFO" | jq -r '.updated_at // "unknown"' 2>/dev/null || echo "unknown")

echo "   Branch ID: $BRANCH_ID"
echo "   Created: $CREATED_AT"
echo "   Last Updated: $UPDATED_AT"
echo ""

# Check for compute resources
COMPUTE_COUNT=$(echo "$DEFAULTS_INFO" | jq -r '.compute | length // 0' 2>/dev/null || echo "0")
if [ "$COMPUTE_COUNT" != "0" ] && [ "$COMPUTE_COUNT" != "null" ] && [ "$COMPUTE_COUNT" != "" ]; then
    echo -e "${YELLOW}   ⚠️  Compute resources found: $COMPUTE_COUNT${NC}"
    echo "$DEFAULTS_INFO" | jq -r '.compute[]? | "      - \(.name // .id) (\(.type // "unknown"))"' 2>/dev/null || true
    echo ""
    echo -e "${RED}   ⚠️  WARNING: Defaults branch has compute resources!${NC}"
    echo -e "${RED}   Deleting this branch may affect these resources.${NC}"
    echo ""
    HAS_COMPUTE=true
else
    echo -e "${GREEN}   ✅ No compute resources (safe to delete)${NC}"
    echo ""
    HAS_COMPUTE=false
fi

# Check for endpoints
ENDPOINT_COUNT=$(echo "$DEFAULTS_INFO" | jq -r '.endpoints | length // 0' 2>/dev/null || echo "0")
if [ "$ENDPOINT_COUNT" != "0" ] && [ "$ENDPOINT_COUNT" != "null" ] && [ "$ENDPOINT_COUNT" != "" ]; then
    echo -e "${YELLOW}   ⚠️  Endpoints found: $ENDPOINT_COUNT${NC}"
    echo "$DEFAULTS_INFO" | jq -r '.endpoints[]? | "      - \(.host // .id)"' 2>/dev/null || true
    echo ""
    HAS_ENDPOINTS=true
else
    echo -e "${GREEN}   ✅ No endpoints${NC}"
    echo ""
    HAS_ENDPOINTS=false
fi

# Display full branch info
echo -e "${BLUE}Full branch info (JSON):${NC}"
echo "$DEFAULTS_INFO" | jq '.' 2>/dev/null || echo "$DEFAULTS_INFO"
echo ""

# Warning message
echo -e "${YELLOW}⚠️  WARNING: Defaults branch is usually empty — but confirm size/age first${NC}"
echo ""

# Show branch details again before confirmation
echo -e "${BLUE}Summary:${NC}"
echo "   Branch ID: $BRANCH_ID"
echo "   Created: $CREATED_AT"
echo "   Last Updated: $UPDATED_AT"
if [ "$HAS_COMPUTE" = true ]; then
    echo -e "${YELLOW}   Compute Resources: YES (⚠️  WARNING)${NC}"
else
    echo -e "${GREEN}   Compute Resources: NO${NC}"
fi
if [ "$HAS_ENDPOINTS" = true ]; then
    echo -e "${YELLOW}   Endpoints: YES${NC}"
else
    echo -e "${GREEN}   Endpoints: NO${NC}"
fi
echo ""

# Final confirmation
echo -e "${RED}⚠️  This will permanently delete the defaults branch!${NC}"
echo ""
if [ "$HAS_COMPUTE" = true ] || [ "$HAS_ENDPOINTS" = true ]; then
    echo -e "${RED}⚠️  WARNING: Branch has compute resources or endpoints!${NC}"
    echo -e "${RED}   Make sure these are not in use before proceeding.${NC}"
    echo ""
fi

read -p "Type 'DELETE DEFAULTS' to confirm deletion: " confirm
if [[ "$confirm" != "DELETE DEFAULTS" ]]; then
    echo "Aborted."
    exit 0
fi

echo ""
echo -e "${BLUE}Deleting defaults branch...${NC}"

# Delete the branch
if neonctl branches delete defaults 2>/dev/null; then
    echo -e "${GREEN}✅ Defaults branch deletion initiated${NC}"
else
    echo -e "${RED}❌ Error: Failed to delete defaults branch${NC}"
    exit 1
fi

echo ""

# Verify deletion success
echo -e "${BLUE}Verifying deletion...${NC}"
sleep 2

BRANCHES_AFTER=$(neonctl branches list --output json 2>/dev/null || echo "[]")

if echo "$BRANCHES_AFTER" | jq -e ".[] | select(.name == \"defaults\")" &>/dev/null; then
    echo -e "${YELLOW}⚠️  Warning: Defaults branch still appears in list${NC}"
    echo "   Deletion may be in progress (check Neon Console)"
else
    echo -e "${GREEN}✅ Defaults branch no longer appears in branch list${NC}"
    echo -e "${GREEN}✅ Deletion verified${NC}"
fi

echo ""
echo -e "${GREEN}=== Cleanup Complete ===${NC}"
echo ""
echo "Defaults branch has been deleted."
echo "Remaining branches:"
echo "$BRANCHES_AFTER" | jq -r '.[] | "  - \(.name) (ID: \(.id))"' 2>/dev/null || echo "  (Unable to parse branch list)"
