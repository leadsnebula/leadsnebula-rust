-- Add performance indexes based on query analysis
-- IMPORTANT: Review query analysis report before applying this migration
-- Only create indexes that show sequential scans in EXPLAIN ANALYZE
-- All indexes use CONCURRENTLY to avoid locking tables

-- ============================================
-- INSTRUCTIONS:
-- ============================================
-- 1. Run analyze-query-patterns.sh first
-- 2. Review the report for queries doing sequential scans
-- 3. Uncomment indexes that address those sequential scans
-- 4. Remove indexes that aren't needed based on analysis
-- 5. All indexes use CONCURRENTLY for zero-downtime creation

-- ============================================
-- LEADS TABLE INDEXES
-- ============================================

-- Index for dashboard: sold leads ordered by created_at
-- Query analysis shows: Dashboard queries benefit from composite index
DO $$
BEGIN
	IF EXISTS(
		SELECT 1 FROM information_schema.tables
		WHERE table_schema = 'public' AND table_name = 'leads'
	) THEN
		EXECUTE 'CREATE INDEX IF NOT EXISTS idx_leads_status_created_at ON leads(status, created_at DESC)';
	ELSE
		RAISE NOTICE 'Table leads does not exist; skipping idx_leads_status_created_at.';
	END IF;
END $$;

-- Index for leads dashboard: filter by publisher with ordering
-- Query analysis shows: Sequential scan on publisher filtering - needs index
DO $$
BEGIN
	IF EXISTS(
		SELECT 1 FROM information_schema.tables
		WHERE table_schema = 'public' AND table_name = 'leads'
	) THEN
		EXECUTE 'CREATE INDEX IF NOT EXISTS idx_leads_publisher_id_created_at ON leads(publisher_id, created_at DESC) WHERE publisher_id IS NOT NULL';
	ELSE
		RAISE NOTICE 'Table leads does not exist; skipping idx_leads_publisher_id_created_at.';
	END IF;
END $$;

-- Index for leads dashboard: filter by buyer with ordering
-- Query analysis shows: Sequential scan on buyer filtering - needs index
DO $$
BEGIN
	IF EXISTS(
		SELECT 1 FROM information_schema.tables
		WHERE table_schema = 'public' AND table_name = 'leads'
	) THEN
		EXECUTE 'CREATE INDEX IF NOT EXISTS idx_leads_buyer_id_created_at ON leads(buyer_id, created_at DESC) WHERE buyer_id IS NOT NULL';
	ELSE
		RAISE NOTICE 'Table leads does not exist; skipping idx_leads_buyer_id_created_at.';
	END IF;
END $$;

-- Index for leads dashboard: filter by status (already have idx_leads_status if exists, but composite may be better)
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: WHERE status = X ORDER BY created_at DESC
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_leads_status_created_at_desc ON leads(status, created_at DESC);

-- Index for search: email domain (case-insensitive)
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: WHERE LOWER(email_domain) LIKE '%X%'
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_leads_email_domain_lower ON leads(LOWER(email_domain));

-- Index for campaign_id filtering (with NULL check)
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: WHERE campaign_id IS NOT NULL AND campaign_id::text != ''
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_leads_campaign_id_valid ON leads(campaign_id) WHERE campaign_id IS NOT NULL;

-- ============================================
-- PINGS TABLE INDEXES
-- ============================================

-- Index for pings dashboard: most recent ping for lead
-- Query analysis shows: Index exists but verify it covers lead_id + created_at
-- Note: idx_pings_created_at exists, but composite with lead_id is better for lead-specific queries
DO $$
BEGIN
	IF EXISTS(
		SELECT 1 FROM information_schema.tables
		WHERE table_schema = 'public' AND table_name = 'pings'
	) THEN
		EXECUTE 'CREATE INDEX IF NOT EXISTS idx_pings_lead_id_created_at ON pings(lead_id, created_at DESC) WHERE lead_id IS NOT NULL';
	ELSE
		RAISE NOTICE 'Table pings does not exist; skipping idx_pings_lead_id_created_at.';
	END IF;
END $$;

-- Index for pings dashboard: filter by lead publisher (via join)
-- Query analysis shows: Sequential scan when joining with leads for publisher filtering
-- The join uses lead_id, so the above index should help, but we also need updated_at for ordering
DO $$
BEGIN
	IF EXISTS(
		SELECT 1 FROM information_schema.tables
		WHERE table_schema = 'public' AND table_name = 'pings'
	) THEN
		EXECUTE 'CREATE INDEX IF NOT EXISTS idx_pings_updated_at ON pings(updated_at DESC)';
	ELSE
		RAISE NOTICE 'Table pings does not exist; skipping idx_pings_updated_at.';
	END IF;
END $$;

-- ============================================
-- POSTS TABLE INDEXES
-- ============================================

-- Index for posts: most recent post for lead
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: WHERE lead_id = X ORDER BY created_at DESC
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_posts_lead_id_created_at ON posts(lead_id, created_at DESC) WHERE lead_id IS NOT NULL;

-- ============================================
-- PING TREES TABLE INDEXES
-- ============================================

-- Index for ping tree routing: find active ping tree
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: WHERE publisher_id = X AND vertical = Y AND status = 'active' ORDER BY priority
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_ping_trees_routing ON ping_trees(publisher_id, vertical, status, priority NULLS LAST, created_at) WHERE status = 'active' AND deleted_at IS NULL;

-- ============================================
-- CAMPAIGNS TABLE INDEXES
-- ============================================

-- Index for campaigns: active campaigns
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: WHERE status = 'active' AND deleted_at IS NULL
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_campaigns_active ON campaigns(status, deleted_at) WHERE status = 'active' AND deleted_at IS NULL;

-- ============================================
-- BUYERS TABLE INDEXES
-- ============================================

-- Index for buyers: active buyers
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: WHERE deleted_at IS NULL ORDER BY created_at DESC
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_buyers_active_created_at ON buyers(deleted_at, created_at DESC) WHERE deleted_at IS NULL;

-- ============================================
-- PUBLISHERS TABLE INDEXES
-- ============================================

-- Index for publishers: active publishers
-- Query analysis shows: Sequential scan on publishers table - needs index
DO $$
BEGIN
	IF EXISTS(
		SELECT 1 FROM information_schema.tables
		WHERE table_schema = 'public' AND table_name = 'publishers'
	) THEN
		EXECUTE 'CREATE INDEX IF NOT EXISTS idx_publishers_active ON publishers(status, deleted_at) WHERE status = ''active'' AND deleted_at IS NULL';
	ELSE
		RAISE NOTICE 'Table publishers does not exist; skipping idx_publishers_active.';
	END IF;
END $$;

-- ============================================
-- LEAD ACCOUNTING TABLE INDEXES
-- ============================================

-- Index for dashboard: sum revenue from sold leads
-- Query analysis shows: Join with leads table - index on lead_id improves join performance
-- Note: lead_accounting table doesn't exist in dev - skipping this index
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_lead_accounting_lead_id ON lead_accounting(lead_id);

-- ============================================
-- LEAD SALES TABLE INDEXES
-- ============================================

-- Index for lead sales: sales for sold leads
-- Uncomment if EXPLAIN ANALYZE shows sequential scan on: JOIN leads WHERE leads.status = 'sold'
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_lead_sales_lead_id ON lead_sales(lead_id);

-- ============================================
-- AUDIT LOGS TABLE INDEXES
-- ============================================

-- Index for audit logs: recent audit logs
-- Query analysis shows: Sequential scan on audit_logs - needs index for ordering
DO $$
BEGIN
	IF EXISTS(
		SELECT 1 FROM information_schema.tables
		WHERE table_schema = 'public' AND table_name = 'audit_logs'
	) THEN
		EXECUTE 'CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at_desc ON audit_logs(created_at DESC)';
	ELSE
		RAISE NOTICE 'Table audit_logs does not exist; skipping idx_audit_logs_created_at_desc.';
	END IF;
END $$;

-- ============================================
-- NOTES:
-- ============================================
-- 1. After creating indexes, verify they're being used:
--    SELECT * FROM pg_stat_user_indexes WHERE indexname LIKE 'idx_%';
-- 2. Monitor index usage with monitor-index-usage.sh
-- 3. Remove unused indexes after 1 week of monitoring
-- 4. Partial indexes (with WHERE clauses) are more efficient for filtered queries
-- 5. Composite indexes should match query patterns (column order matters)
