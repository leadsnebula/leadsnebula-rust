-- Add GIN indexes for JSONB fields (conditional - only if query analysis shows need)
-- IMPORTANT: Only create GIN indexes if EXPLAIN ANALYZE shows performance issues
-- GIN indexes are large and slow to create - only use when necessary

-- ============================================
-- INSTRUCTIONS:
-- ============================================
-- 1. Run analyze-query-patterns.sh first
-- 2. Check if queries on JSONB fields show performance issues
-- 3. Only uncomment GIN indexes for JSONB fields that are:
--    - Queried frequently
--    - Show high execution time in EXPLAIN ANALYZE
--    - Have large JSONB values
-- 4. GIN indexes use CONCURRENTLY for zero-downtime creation

-- ============================================
-- BUYERS TABLE JSONB INDEXES
-- ============================================

-- Index for buyers.contact_info JSONB field
-- Uncomment if EXPLAIN ANALYZE shows performance issues querying contact_info
-- Example query that would benefit: WHERE contact_info @> '{"email": "test@example.com"}'
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_buyers_contact_info_gin ON buyers USING GIN (contact_info);

-- Index for buyers.documents JSONB field
-- Uncomment if EXPLAIN ANALYZE shows performance issues querying documents
-- Example query that would benefit: WHERE documents @> '[{"type": "contract"}]'
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_buyers_documents_gin ON buyers USING GIN (documents);

-- ============================================
-- LEADS TABLE JSONB INDEXES
-- ============================================

-- Index for leads.vertical_data JSONB field
-- Uncomment if EXPLAIN ANALYZE shows performance issues querying vertical_data
-- Example query that would benefit: WHERE vertical_data @> '{"property_type": "single_family"}'
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_leads_vertical_data_gin ON leads USING GIN (vertical_data);

-- Index for leads.utm_params JSONB field
-- Uncomment if EXPLAIN ANALYZE shows performance issues querying utm_params for analytics
-- Example query that would benefit: WHERE utm_params @> '{"source": "google"}'
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_leads_utm_params_gin ON leads USING GIN (utm_params);

-- ============================================
-- OTHER JSONB FIELDS
-- ============================================

-- Add indexes for other JSONB fields as needed based on query analysis
-- Common JSONB fields to check:
-- - ping_payloads.response_payload_encrypted (if queried)
-- - post_payloads.response_payload_encrypted (if queried)
-- - buyer_integrations.config (if queried)
-- - Any other JSONB columns that show up in slow queries

-- ============================================
-- NOTES:
-- ============================================
-- 1. GIN indexes are large - monitor disk space before creating
-- 2. GIN index creation can be slow on large tables
-- 3. Use CONCURRENTLY to avoid locking tables
-- 4. GIN indexes support operators: @>, ?, ?&, ?|, @?, @@
-- 5. Only create if queries actually use these operators on JSONB fields
-- 6. Consider jsonb_path_ops for better performance if only using @> operator:
--    CREATE INDEX ... USING GIN (column jsonb_path_ops);
