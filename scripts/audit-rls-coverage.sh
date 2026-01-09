#!/bin/bash
# Audit RLS (Row Level Security) coverage on all tables
# Identifies tables missing proper isolation policies

set -euo pipefail

ENVIRONMENT="${1:-prod}"
REPORT_FILE="${2:-rls-audit-report-$(date +%Y%m%d-%H%M%S).txt}"

# Normalize environment name
normalize_env() {
    case "$1" in
        "production") echo "prod" ;;
        "development") echo "dev" ;;
        *) echo "$1" ;;
    esac
}

ENV_NORMALIZED=$(normalize_env "$ENVIRONMENT")

echo "🔍 RLS Audit for: $ENVIRONMENT (normalized: $ENV_NORMALIZED)"
echo "📄 Report will be saved to: $REPORT_FILE"
echo ""

# Check AWS credentials
if [ -z "${AWS_ACCESS_KEY_ID:-}" ] || [ -z "${AWS_SECRET_ACCESS_KEY:-}" ] || [ -z "${AWS_REGION:-}" ]; then
    echo "❌ ERROR: AWS credentials not set"
    echo "   Required: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION"
    exit 1
fi

# Get connection string from SSM
CONN_PARAM="/leadsnebula/${ENV_NORMALIZED}/rust/db/connection_url_direct"
echo "📡 Fetching connection string from SSM: $CONN_PARAM"

CONN_STRING=$(aws ssm get-parameter --name "$CONN_PARAM" --region "${AWS_REGION}" --with-decryption --query 'Parameter.Value' --output text 2>/dev/null)

if [ -z "$CONN_STRING" ]; then
    echo "❌ ERROR: Failed to fetch connection string from SSM"
    exit 1
fi

# Mask password in output
CONN_STRING_MASKED=$(echo "$CONN_STRING" | sed 's/:[^:@]*@/:***@/')
echo "✅ Connected to: $CONN_STRING_MASKED"
echo ""

# Check if psql is available
if ! command -v psql &> /dev/null; then
    echo "❌ ERROR: psql not found. Please install PostgreSQL client tools."
    exit 1
fi

# Initialize report file
{
    echo "=========================================="
    echo "RLS Audit Report"
    echo "Environment: $ENVIRONMENT"
    echo "Generated: $(date)"
    echo "=========================================="
    echo ""
} > "$REPORT_FILE"

echo "📊 Analyzing RLS coverage..."
echo ""

# Get all tables in public schema
TABLES=$(psql "$CONN_STRING" -t -c "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename;" | tr -d ' ')

# Tables that should have instance-level isolation
INSTANCE_ISOLATED_TABLES=(
    "instances"
    "instance_users"
    "publishers"
    "buyers"
    "campaigns"
    "ping_trees"
    "ping_tree_campaigns"
    "buyer_integrations"
    "buyer_integration_credentials"
    "encryption_key_versions"
    "password_histories"
    "webauthn_credentials"
    "user_otp_settings"
)

# Tables that should have publisher-level isolation (via leads)
PUBLISHER_ISOLATED_TABLES=(
    "leads"
    "pings"
    "posts"
    "lead_sales"
    "lead_accounting"
    "lead_revenues"
    "ping_payloads"
    "post_payloads"
)

# Tables that might not need isolation (system/audit tables)
SYSTEM_TABLES=(
    "_sqlx_migrations"
    "audit_logs"
    "pii_access_logs"
    "encryption_rotation_jobs"
)

# Function to check if RLS is enabled
check_rls_enabled() {
    local table_name="$1"
    psql "$CONN_STRING" -t -c "SELECT relrowsecurity FROM pg_class WHERE relname = '$table_name' AND relnamespace = (SELECT oid FROM pg_namespace WHERE nspname = 'public');" | tr -d ' ' | grep -q 't'
}

# Function to get policy count
get_policy_count() {
    local table_name="$1"
    psql "$CONN_STRING" -t -c "SELECT COUNT(*) FROM pg_policies WHERE tablename = '$table_name';" | tr -d ' '
}

# Function to get policy names
get_policy_names() {
    local table_name="$1"
    psql "$CONN_STRING" -t -c "SELECT policyname FROM pg_policies WHERE tablename = '$table_name' ORDER BY policyname;" | tr -d ' ' | tr '\n' ',' | sed 's/,$//'
}

# Function to check if table has isolation policy
has_isolation_policy() {
    local table_name="$1"
    local policies=$(get_policy_names "$table_name")
    
    # Check for consolidated or isolation policies
    echo "$policies" | grep -qE "(consolidated_access|publisher_isolation|instance_isolation|buyer_isolation)" || \
    echo "$policies" | grep -qE "(current_setting|app\.current_publisher_id|app\.current_instance_id|app\.current_buyer_id)"
}

# Analyze each table
TABLES_WITHOUT_RLS=()
TABLES_WITHOUT_POLICIES=()
TABLES_WITHOUT_ISOLATION=()
TABLES_WITH_ISOLATION=()

{
    echo "Table Analysis:"
    echo "==============="
    echo ""
} >> "$REPORT_FILE"

for table in $TABLES; do
    if [ -z "$table" ]; then
        continue
    fi
    
    echo "  Analyzing: $table"
    
    RLS_ENABLED=false
    POLICY_COUNT=0
    POLICIES=""
    HAS_ISOLATION=false
    
    if check_rls_enabled "$table"; then
        RLS_ENABLED=true
        POLICY_COUNT=$(get_policy_count "$table")
        POLICIES=$(get_policy_names "$table")
        
        if has_isolation_policy "$table"; then
            HAS_ISOLATION=true
            TABLES_WITH_ISOLATION+=("$table")
        else
            TABLES_WITHOUT_ISOLATION+=("$table")
        fi
    else
        TABLES_WITHOUT_RLS+=("$table")
    fi
    
    if [ "$RLS_ENABLED" = true ] && [ "$POLICY_COUNT" -eq 0 ]; then
        TABLES_WITHOUT_POLICIES+=("$table")
    fi
    
    {
        echo "----------------------------------------"
        echo "Table: $table"
        echo "  RLS Enabled: $RLS_ENABLED"
        echo "  Policy Count: $POLICY_COUNT"
        if [ -n "$POLICIES" ]; then
            echo "  Policies: $POLICIES"
        fi
        echo "  Has Isolation: $HAS_ISOLATION"
        
        # Categorize table
        if [[ " ${INSTANCE_ISOLATED_TABLES[@]} " =~ " ${table} " ]]; then
            echo "  Expected: Instance-level isolation"
        elif [[ " ${PUBLISHER_ISOLATED_TABLES[@]} " =~ " ${table} " ]]; then
            echo "  Expected: Publisher-level isolation"
        elif [[ " ${SYSTEM_TABLES[@]} " =~ " ${table} " ]]; then
            echo "  Expected: System/audit table (may not need isolation)"
        else
            echo "  Expected: Unknown (needs review)"
        fi
        echo ""
    } >> "$REPORT_FILE"
done

# Generate summary
{
    echo ""
    echo "=========================================="
    echo "Summary"
    echo "=========================================="
    echo ""
    echo "Tables without RLS: ${#TABLES_WITHOUT_RLS[@]}"
    if [ ${#TABLES_WITHOUT_RLS[@]} -gt 0 ]; then
        for table in "${TABLES_WITHOUT_RLS[@]}"; do
            echo "  - $table"
        done
    fi
    echo ""
    
    echo "Tables without policies: ${#TABLES_WITHOUT_POLICIES[@]}"
    if [ ${#TABLES_WITHOUT_POLICIES[@]} -gt 0 ]; then
        for table in "${TABLES_WITHOUT_POLICIES[@]}"; do
            echo "  - $table"
        done
    fi
    echo ""
    
    echo "Tables without isolation policies: ${#TABLES_WITHOUT_ISOLATION[@]}"
    if [ ${#TABLES_WITHOUT_ISOLATION[@]} -gt 0 ]; then
        for table in "${TABLES_WITHOUT_ISOLATION[@]}"; do
            echo "  - $table"
        done
    fi
    echo ""
    
    echo "Tables with proper isolation: ${#TABLES_WITH_ISOLATION[@]}"
    if [ ${#TABLES_WITH_ISOLATION[@]} -gt 0 ]; then
        for table in "${TABLES_WITH_ISOLATION[@]}"; do
            echo "  ✅ $table"
        done
    fi
    echo ""
    
    echo "=========================================="
    echo "Recommendations"
    echo "=========================================="
    echo ""
    
    if [ ${#TABLES_WITHOUT_RLS[@]} -gt 0 ]; then
        echo "⚠️  Enable RLS on tables without it:"
        for table in "${TABLES_WITHOUT_RLS[@]}"; do
            echo "   ALTER TABLE $table ENABLE ROW LEVEL SECURITY;"
        done
        echo ""
    fi
    
    if [ ${#TABLES_WITHOUT_ISOLATION[@]} -gt 0 ]; then
        echo "⚠️  Add isolation policies to:"
        for table in "${TABLES_WITHOUT_ISOLATION[@]}"; do
            if [[ " ${INSTANCE_ISOLATED_TABLES[@]} " =~ " ${table} " ]]; then
                echo "   - $table (needs instance-level isolation)"
            elif [[ " ${PUBLISHER_ISOLATED_TABLES[@]} " =~ " ${table} " ]]; then
                echo "   - $table (needs publisher-level isolation)"
            else
                echo "   - $table (needs review for appropriate isolation)"
            fi
        done
    fi
    echo ""
} >> "$REPORT_FILE"

echo ""
echo "✅ RLS audit complete!"
echo "📄 Full report saved to: $REPORT_FILE"
echo ""
echo "📋 Summary:"
echo "   - Tables without RLS: ${#TABLES_WITHOUT_RLS[@]}"
echo "   - Tables without policies: ${#TABLES_WITHOUT_POLICIES[@]}"
echo "   - Tables without isolation: ${#TABLES_WITHOUT_ISOLATION[@]}"
echo "   - Tables with proper isolation: ${#TABLES_WITH_ISOLATION[@]}"
echo ""
