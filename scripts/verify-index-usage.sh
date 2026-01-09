#!/bin/bash
# Verify index usage for newly created indexes
# Quick check to see if indexes are being used in production queries
# For comprehensive analysis, use monitor-index-usage.sh

set -euo pipefail

ENVIRONMENT="${1:-prod}"
INDEX_NAME="${2:-}"

# Normalize environment name
normalize_env() {
    case "$1" in
        "production") echo "prod" ;;
        "development") echo "dev" ;;
        *) echo "$1" ;;
    esac
}

ENV_NORMALIZED=$(normalize_env "$ENVIRONMENT")

echo "🔍 Index Usage Verification for: $ENVIRONMENT (normalized: $ENV_NORMALIZED)"
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

# If specific index name provided, check only that index
if [ -n "$INDEX_NAME" ]; then
    echo "📊 Checking usage for index: $INDEX_NAME"
    echo ""
    
    psql "$CONN_STRING" -c "
        SELECT 
            schemaname,
            tablename,
            indexname,
            idx_scan as index_scans,
            idx_tup_read as tuples_read,
            idx_tup_fetch as tuples_fetched,
            pg_size_pretty(pg_relation_size(indexrelid)) as index_size,
            CASE 
                WHEN idx_scan = 0 THEN '⚠️  UNUSED'
                WHEN idx_scan < 10 THEN '⚠️  LOW USAGE'
                ELSE '✅ IN USE'
            END as status
        FROM pg_stat_user_indexes
        WHERE indexname = '$INDEX_NAME'
        ORDER BY idx_scan DESC;
    "
    
    echo ""
    echo "💡 Interpretation:"
    echo "   - idx_scan = 0: Index has never been used (consider removing after 2 weeks)"
    echo "   - idx_scan < 10: Low usage (monitor for specific use cases)"
    echo "   - idx_scan >= 10: Index is being used"
    exit 0
fi

# Otherwise, show summary of all indexes
echo "📊 Index Usage Summary"
echo ""

# Summary statistics
echo "=========================================="
echo "Overall Statistics"
echo "=========================================="
psql "$CONN_STRING" -c "
    SELECT 
        COUNT(*) as total_indexes,
        COUNT(*) FILTER (WHERE idx_scan = 0) as unused_indexes,
        COUNT(*) FILTER (WHERE idx_scan > 0 AND idx_scan < 10) as low_usage_indexes,
        COUNT(*) FILTER (WHERE idx_scan >= 10) as active_indexes,
        pg_size_pretty(SUM(pg_relation_size(indexrelid))) as total_index_size,
        pg_size_pretty(SUM(pg_relation_size(indexrelid)) FILTER (WHERE idx_scan = 0)) as unused_index_size
    FROM pg_stat_user_indexes;
"

echo ""
echo "=========================================="
echo "Unused Indexes (idx_scan = 0)"
echo "=========================================="
echo "⚠️  These indexes have never been scanned."
echo "    Monitor for at least 1 week before considering removal."
echo ""
psql "$CONN_STRING" -c "
    SELECT 
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
    ORDER BY pg_relation_size(indexrelid) DESC
    LIMIT 20;
"

echo ""
echo "=========================================="
echo "Recently Created Indexes (New Indexes)"
echo "=========================================="
echo "Indexes matching pattern 'idx_*' created in recent migrations:"
echo ""
psql "$CONN_STRING" -c "
    SELECT 
        tablename,
        indexname,
        idx_scan as index_scans,
        idx_tup_read as tuples_read,
        pg_size_pretty(pg_relation_size(indexrelid)) as index_size,
        CASE 
            WHEN idx_scan = 0 THEN '⚠️  UNUSED'
            WHEN idx_scan < 10 THEN '⚠️  LOW USAGE'
            ELSE '✅ IN USE'
        END as status
    FROM pg_stat_user_indexes
    WHERE indexname LIKE 'idx_%'
    ORDER BY idx_scan ASC, tablename, indexname;
"

echo ""
echo "=========================================="
echo "Recommendations"
echo "=========================================="
echo ""
echo "1. Unused indexes (idx_scan = 0):"
echo "   - Monitor for at least 1 week before removal"
echo "   - Never drop indexes supporting UNIQUE or PRIMARY KEY constraints"
echo "   - Use DROP INDEX CONCURRENTLY for production"
echo ""
echo "2. Low-usage indexes (idx_scan < 10):"
echo "   - May be used for specific queries (check query logs)"
echo "   - Consider keeping if they support critical but infrequent operations"
echo ""
echo "3. For detailed analysis, run:"
echo "   ./scripts/monitor-index-usage.sh $ENVIRONMENT"
echo ""
echo "4. To check a specific index:"
echo "   ./scripts/verify-index-usage.sh $ENVIRONMENT <index_name>"
echo ""
