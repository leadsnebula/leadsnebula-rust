#!/bin/bash
# Analyze query patterns on production database
# Runs EXPLAIN ANALYZE on common queries to identify missing indexes and performance issues

set -euo pipefail

ENVIRONMENT="${1:-prod}"
REPORT_FILE="${2:-query-analysis-report-$(date +%Y%m%d-%H%M%S).txt}"

# Normalize environment name
normalize_env() {
    case "$1" in
        "production") echo "prod" ;;
        "development") echo "dev" ;;
        *) echo "$1" ;;
    esac
}

ENV_NORMALIZED=$(normalize_env "$ENVIRONMENT")

echo "🔍 Query Pattern Analysis for: $ENVIRONMENT (normalized: $ENV_NORMALIZED)"
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
    echo "Query Pattern Analysis Report"
    echo "Environment: $ENVIRONMENT"
    echo "Generated: $(date)"
    echo "=========================================="
    echo ""
} > "$REPORT_FILE"

# Function to run EXPLAIN ANALYZE and append to report
analyze_query() {
    local query_name="$1"
    local query_sql="$2"
    
    echo "📊 Analyzing: $query_name"
    echo "   Query: ${query_sql:0:80}..."
    
    {
        echo "=========================================="
        echo "Query: $query_name"
        echo "----------------------------------------"
        echo "SQL:"
        echo "$query_sql"
        echo ""
        echo "EXPLAIN ANALYZE:"
        echo "----------------------------------------"
    } >> "$REPORT_FILE"
    
    # Run EXPLAIN ANALYZE
    if psql "$CONN_STRING" -c "EXPLAIN (ANALYZE, BUFFERS, VERBOSE) $query_sql" >> "$REPORT_FILE" 2>&1; then
        echo "   ✅ Completed"
    else
        echo "   ⚠️  Error (check report for details)"
    fi
    
    {
        echo ""
        echo ""
    } >> "$REPORT_FILE"
}

# Common query patterns to analyze

echo "Starting query analysis..."
echo ""

# 1. Dashboard: Sold leads with pagination
analyze_query \
    "Dashboard: Sold leads (paginated)" \
    "SELECT l.* FROM leads l WHERE l.status = 'sold' ORDER BY l.created_at DESC LIMIT 50"

# 2. Dashboard: Count sold leads
analyze_query \
    "Dashboard: Count sold leads" \
    "SELECT COUNT(*) FROM leads WHERE status = 'sold'"

# 3. Dashboard: Sum revenue from sold leads
analyze_query \
    "Dashboard: Sum revenue from sold leads" \
    "SELECT SUM(la.price) FROM lead_accounting la INNER JOIN leads l ON la.lead_id = l.uuid WHERE l.status = 'sold'"

# 4. Leads Dashboard: Filter by publisher
analyze_query \
    "Leads Dashboard: Filter by publisher" \
    "SELECT l.* FROM leads l WHERE l.publisher_id IS NOT NULL ORDER BY l.created_at DESC LIMIT 100"

# 5. Leads Dashboard: Filter by buyer
analyze_query \
    "Leads Dashboard: Filter by buyer" \
    "SELECT l.* FROM leads l WHERE l.buyer_id IS NOT NULL ORDER BY l.created_at DESC LIMIT 100"

# 6. Leads Dashboard: Filter by status
analyze_query \
    "Leads Dashboard: Filter by status" \
    "SELECT l.* FROM leads l WHERE l.status = 'processing' ORDER BY l.created_at DESC LIMIT 100"

# 7. Leads Dashboard: Search by encrypted email (deterministic)
analyze_query \
    "Leads Dashboard: Search by encrypted email" \
    "SELECT l.* FROM leads l WHERE l.email_encrypted = 'test@example.com' LIMIT 10"

# 8. Leads Dashboard: Search by email domain
analyze_query \
    "Leads Dashboard: Search by email domain" \
    "SELECT l.* FROM leads l WHERE LOWER(l.email_domain) LIKE '%example%' LIMIT 10"

# 9. Leads Dashboard: Filter by campaign_id (with NULL check)
analyze_query \
    "Leads Dashboard: Filter valid campaign_ids" \
    "SELECT l.* FROM leads l WHERE (l.campaign_id IS NULL OR l.campaign_id::text != '') ORDER BY l.created_at DESC LIMIT 100"

# 10. Pings: Get most recent ping for a lead
analyze_query \
    "Pings: Most recent ping for lead" \
    "SELECT p.* FROM pings p WHERE p.lead_id IS NOT NULL ORDER BY p.created_at DESC LIMIT 1"

# 11. Pings Dashboard: Filter by lead publisher
analyze_query \
    "Pings Dashboard: Filter by lead publisher" \
    "SELECT p.* FROM pings p INNER JOIN leads l ON p.lead_id = l.uuid WHERE l.publisher_id IS NOT NULL ORDER BY p.updated_at DESC LIMIT 50"

# 12. Pings Dashboard: Filter by lead buyer
analyze_query \
    "Pings Dashboard: Filter by lead buyer" \
    "SELECT p.* FROM pings p INNER JOIN leads l ON p.lead_id = l.uuid WHERE l.buyer_id IS NOT NULL ORDER BY p.updated_at DESC LIMIT 50"

# 13. Posts: Get most recent post for a lead
analyze_query \
    "Posts: Most recent post for lead" \
    "SELECT p.* FROM posts p WHERE p.lead_id IS NOT NULL ORDER BY p.created_at DESC LIMIT 1"

# 14. Ping Trees: Find active ping tree for routing
analyze_query \
    "Ping Trees: Find active ping tree for routing" \
    "SELECT pt.* FROM ping_trees pt WHERE pt.publisher_id IS NOT NULL AND pt.vertical = 'solar' AND pt.status = 'active' AND pt.deleted_at IS NULL ORDER BY pt.priority ASC NULLS LAST, pt.created_at ASC LIMIT 1"

# 15. Campaigns: Active campaigns
analyze_query \
    "Campaigns: Active campaigns" \
    "SELECT c.* FROM campaigns c WHERE c.status = 'active' AND c.deleted_at IS NULL ORDER BY c.created_at DESC LIMIT 100"

# 16. Buyers: Active buyers
analyze_query \
    "Buyers: Active buyers" \
    "SELECT b.* FROM buyers b WHERE b.deleted_at IS NULL ORDER BY b.created_at DESC LIMIT 100"

# 17. Publishers: Active publishers
analyze_query \
    "Publishers: Active publishers" \
    "SELECT p.* FROM publishers p WHERE p.status = 'active' AND p.deleted_at IS NULL ORDER BY p.created_at DESC LIMIT 100"

# 18. Lead Sales: Get sales for sold leads
analyze_query \
    "Lead Sales: Sales for sold leads" \
    "SELECT ls.* FROM lead_sales ls INNER JOIN leads l ON ls.lead_id = l.uuid WHERE l.status = 'sold' LIMIT 100"

# 19. Lead Accounting: Get accounting for sold leads
analyze_query \
    "Lead Accounting: Accounting for sold leads" \
    "SELECT la.* FROM lead_accounting la INNER JOIN leads l ON la.lead_id = l.uuid WHERE l.status = 'sold' LIMIT 100"

# 20. Audit Logs: Recent audit logs
analyze_query \
    "Audit Logs: Recent audit logs" \
    "SELECT al.* FROM audit_logs al ORDER BY al.created_at DESC LIMIT 100"

echo ""
echo "✅ Query analysis complete!"
echo "📄 Full report saved to: $REPORT_FILE"
echo ""
echo "📋 Summary of what to check:"
echo "   - Look for 'Seq Scan' (sequential scan) - indicates missing index"
echo "   - Check 'Execution Time' - queries > 100ms may need optimization"
echo "   - Look for 'Index Scan' or 'Index Only Scan' - good, indexes are being used"
echo "   - Check 'Rows Removed by Filter' - high values indicate inefficient filtering"
echo "   - Look for 'Buffers: shared hit/read' - high read values indicate cache misses"
echo ""
