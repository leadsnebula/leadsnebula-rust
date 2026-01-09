#!/bin/bash
# Monitor index usage and identify unused indexes
# Tracks index scans, tuple reads, and identifies candidates for removal

set -euo pipefail

ENVIRONMENT="${1:-prod}"
REPORT_FILE="${2:-index-usage-report-$(date +%Y%m%d-%H%M%S).txt}"

# Normalize environment name
normalize_env() {
    case "$1" in
        "production") echo "prod" ;;
        "development") echo "dev" ;;
        *) echo "$1" ;;
    esac
}

ENV_NORMALIZED=$(normalize_env "$ENVIRONMENT")

echo "🔍 Index Usage Monitoring for: $ENVIRONMENT (normalized: $ENV_NORMALIZED)"
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
    echo "Index Usage Monitoring Report"
    echo "Environment: $ENVIRONMENT"
    echo "Generated: $(date)"
    echo "=========================================="
    echo ""
    echo "Note: Index usage statistics are cumulative since the last"
    echo "      PostgreSQL statistics reset. Unused indexes (idx_scan = 0)"
    echo "      should be monitored for at least 1 week before removal."
    echo ""
} > "$REPORT_FILE"

echo "📊 Analyzing index usage..."
echo ""

# Get index usage statistics
{
    echo "=========================================="
    echo "Index Usage Statistics"
    echo "=========================================="
    echo ""
    echo "All indexes sorted by scan count (descending):"
    echo ""
    psql "$CONN_STRING" -c "
        SELECT 
            schemaname,
            tablename,
            indexname,
            idx_scan as index_scans,
            idx_tup_read as tuples_read,
            idx_tup_fetch as tuples_fetched,
            pg_size_pretty(pg_relation_size(indexrelid)) as index_size
        FROM pg_stat_user_indexes
        ORDER BY idx_scan DESC, tablename, indexname;
    " >> "$REPORT_FILE"
    
    echo ""
    echo "=========================================="
    echo "Unused Indexes (idx_scan = 0)"
    echo "=========================================="
    echo ""
    echo "⚠️  WARNING: These indexes have never been scanned."
    echo "    Monitor for at least 1 week before considering removal."
    echo "    Some indexes may be used for constraints (UNIQUE, PRIMARY KEY)."
    echo ""
    psql "$CONN_STRING" -c "
        SELECT 
            schemaname,
            tablename,
            indexname,
            pg_size_pretty(pg_relation_size(indexrelid)) as index_size,
            CASE 
                WHEN indexdef LIKE '%UNIQUE%' THEN 'UNIQUE constraint'
                WHEN indexdef LIKE '%PRIMARY KEY%' THEN 'PRIMARY KEY'
                ELSE 'Regular index'
            END as index_type
        FROM pg_stat_user_indexes
        JOIN pg_indexes ON pg_stat_user_indexes.indexname = pg_indexes.indexname
        WHERE idx_scan = 0
        ORDER BY pg_relation_size(indexrelid) DESC, tablename, indexname;
    " >> "$REPORT_FILE"
    
    echo ""
    echo "=========================================="
    echo "Low-Usage Indexes (idx_scan < 10)"
    echo "=========================================="
    echo ""
    echo "Indexes with very low usage. Review if they're still needed."
    echo ""
    psql "$CONN_STRING" -c "
        SELECT 
            schemaname,
            tablename,
            indexname,
            idx_scan as index_scans,
            pg_size_pretty(pg_relation_size(indexrelid)) as index_size
        FROM pg_stat_user_indexes
        WHERE idx_scan > 0 AND idx_scan < 10
        ORDER BY idx_scan ASC, pg_relation_size(indexrelid) DESC, tablename, indexname;
    " >> "$REPORT_FILE"
    
    echo ""
    echo "=========================================="
    echo "Large Unused Indexes (Potential Space Savings)"
    echo "=========================================="
    echo ""
    echo "Unused indexes sorted by size (largest first)."
    echo "These are the best candidates for removal if confirmed unused."
    echo ""
    psql "$CONN_STRING" -c "
        SELECT 
            schemaname,
            tablename,
            indexname,
            pg_size_pretty(pg_relation_size(indexrelid)) as index_size,
            pg_relation_size(indexrelid) as size_bytes,
            CASE 
                WHEN indexdef LIKE '%UNIQUE%' THEN 'UNIQUE constraint'
                WHEN indexdef LIKE '%PRIMARY KEY%' THEN 'PRIMARY KEY'
                ELSE 'Regular index'
            END as index_type
        FROM pg_stat_user_indexes
        JOIN pg_indexes ON pg_stat_user_indexes.indexname = pg_indexes.indexname
        WHERE idx_scan = 0
        ORDER BY pg_relation_size(indexrelid) DESC
        LIMIT 20;
    " >> "$REPORT_FILE"
    
    echo ""
    echo "=========================================="
    echo "Index Size Summary"
    echo "=========================================="
    echo ""
    psql "$CONN_STRING" -c "
        SELECT 
            COUNT(*) as total_indexes,
            COUNT(*) FILTER (WHERE idx_scan = 0) as unused_indexes,
            COUNT(*) FILTER (WHERE idx_scan > 0 AND idx_scan < 10) as low_usage_indexes,
            pg_size_pretty(SUM(pg_relation_size(indexrelid))) as total_index_size,
            pg_size_pretty(SUM(pg_relation_size(indexrelid)) FILTER (WHERE idx_scan = 0)) as unused_index_size
        FROM pg_stat_user_indexes;
    " >> "$REPORT_FILE"
    
    echo ""
    echo "=========================================="
    echo "Recommendations"
    echo "=========================================="
    echo ""
    echo "1. Monitor unused indexes for at least 1 week before removal"
    echo "2. Never drop indexes that support UNIQUE or PRIMARY KEY constraints"
    echo "3. Consider dropping large unused indexes to save space"
    echo "4. Review low-usage indexes - they may be needed for specific queries"
    echo "5. Use DROP INDEX CONCURRENTLY for production to avoid locks"
    echo ""
    
} >> "$REPORT_FILE"

echo "✅ Index usage analysis complete!"
echo "📄 Full report saved to: $REPORT_FILE"
echo ""
echo "📋 Key metrics:"
UNUSED_COUNT=$(psql "$CONN_STRING" -t -c "SELECT COUNT(*) FROM pg_stat_user_indexes WHERE idx_scan = 0;" | tr -d ' ')
LOW_USAGE_COUNT=$(psql "$CONN_STRING" -t -c "SELECT COUNT(*) FROM pg_stat_user_indexes WHERE idx_scan > 0 AND idx_scan < 10;" | tr -d ' ')
TOTAL_SIZE=$(psql "$CONN_STRING" -t -c "SELECT pg_size_pretty(SUM(pg_relation_size(indexrelid))) FROM pg_stat_user_indexes;" | tr -d ' ')

echo "   - Unused indexes (idx_scan = 0): $UNUSED_COUNT"
echo "   - Low-usage indexes (idx_scan < 10): $LOW_USAGE_COUNT"
echo "   - Total index size: $TOTAL_SIZE"
echo ""
echo "⚠️  Remember: Index usage stats are cumulative. Monitor unused"
echo "   indexes for at least 1 week before removal."
echo ""
