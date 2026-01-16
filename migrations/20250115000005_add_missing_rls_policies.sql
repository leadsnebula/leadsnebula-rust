-- Add missing RLS policies for instance/publisher isolation
-- IMPORTANT: Run audit-rls-coverage.sh first to identify which tables need policies
-- This migration is a template - uncomment and customize based on audit results

-- ============================================
-- RLS POLICIES BASED ON AUDIT RESULTS
-- ============================================
-- This migration adds RLS policies for tables identified in the RLS audit
-- Audit date: 2026-01-09
-- Tables requiring RLS: buyer_integrations, encryption_key_versions, instance_users, instances, password_histories
-- Note: System tables (_sqlx_migrations, schema_migrations) and junction tables (ping_tree_campaigns, publisher_verticals) 
--       are intentionally excluded as they don't need instance-level isolation

-- ============================================
-- INSTANCE-LEVEL ISOLATION POLICIES
-- ============================================
-- These tables should be isolated by instance_id
-- Tables: instances, instance_users, publishers, buyers, campaigns, ping_trees, etc.

-- Instance users table - admin/system only
-- RLS audit shows: instance_users table needs RLS enabled
-- Note: instance_users doesn't have instance_id column - it's a global user table
ALTER TABLE instance_users ENABLE ROW LEVEL SECURITY;

DO $$ BEGIN
    CREATE POLICY instance_users_admin_system_only ON instance_users
        FOR ALL
        USING (
            (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Instances table - admin/system only (instances are top-level, users access via instance_id)
-- RLS audit shows: instances table needs RLS enabled
ALTER TABLE instances ENABLE ROW LEVEL SECURITY;

DO $$ BEGIN
    CREATE POLICY instances_admin_system_only ON instances
        FOR ALL
        USING (
            (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
            OR
            id = (SELECT current_setting('app.current_instance_id', true))::uuid
        );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Buyer zip lists table instance isolation
-- RLS audit shows: buyer_zip_lists table needs RLS enabled and instance isolation
ALTER TABLE buyer_zip_lists ENABLE ROW LEVEL SECURITY;

DO $$ BEGIN
    CREATE POLICY buyer_zip_lists_instance_isolation ON buyer_zip_lists
        FOR ALL
        USING (
            EXISTS (
                SELECT 1 FROM buyers
                WHERE buyers.id = buyer_zip_lists.buyer_id
                AND buyers.instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
            )
            OR
            (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Buyer zip codes table instance isolation (via buyer_zip_lists)
-- RLS audit shows: buyer_zip_codes table needs RLS enabled and instance isolation
ALTER TABLE buyer_zip_codes ENABLE ROW LEVEL SECURITY;

DO $$ BEGIN
    CREATE POLICY buyer_zip_codes_instance_isolation ON buyer_zip_codes
        FOR ALL
        USING (
            EXISTS (
                SELECT 1 FROM buyer_zip_lists bzl
                INNER JOIN buyers b ON b.id = bzl.buyer_id
                WHERE bzl.id = buyer_zip_codes.buyer_zip_list_id
                AND b.instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
            )
            OR
            (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Example: Buyers table instance isolation
-- Uncomment if audit shows buyers table needs instance isolation
/*
CREATE POLICY buyers_instance_isolation ON buyers
    FOR ALL
    USING (
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
    );
*/

-- Example: Campaigns table instance isolation
-- Uncomment if audit shows campaigns table needs instance isolation
/*
CREATE POLICY campaigns_instance_isolation ON campaigns
    FOR ALL
    USING (
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
    );
*/

-- Note: Ping trees already has RLS enabled with proper isolation policies

-- Buyer integrations table - admin/system only
-- RLS audit shows: buyer_integrations table needs RLS enabled
-- Note: buyer_integrations is a template table (not instance-specific), accessed via buyers table
-- ALTER TABLE buyer_integrations ENABLE ROW LEVEL SECURITY;
--
-- CREATE POLICY buyer_integrations_admin_system_only ON buyer_integrations
--     FOR ALL
--     USING (
--         (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
--     );

-- Encryption key versions table - admin/system only
-- RLS audit shows: encryption_key_versions table needs RLS enabled
-- Note: encryption_key_versions doesn't have instance_id column - it's a global encryption key table
-- ALTER TABLE encryption_key_versions ENABLE ROW LEVEL SECURITY;
--
-- CREATE POLICY encryption_key_versions_admin_system_only ON encryption_key_versions
--     FOR ALL
--     USING (
--         (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
--     );

-- Password histories table - admin/system only
-- RLS audit shows: password_histories table needs RLS enabled
-- Note: password_histories references instance_users which doesn't have instance_id
-- ALTER TABLE password_histories ENABLE ROW LEVEL SECURITY;
--
-- CREATE POLICY password_histories_admin_system_only ON password_histories
--     FOR ALL
--     USING (
--         (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
--     );

-- ============================================
-- NOTES:
-- ============================================
-- 1. Publisher-level isolation: leads, pings, posts, lead_sales, lead_accounting already have
--    consolidated RLS policies from previous migrations
-- 2. System tables (_sqlx_migrations, schema_migrations) intentionally excluded
-- 3. Junction tables (ping_tree_campaigns, publisher_verticals) don't need instance isolation
--    as they're accessed via their parent tables which already have RLS

-- ============================================
-- NOTES:
-- ============================================
-- 1. Always test policies on dev database first
-- 2. Ensure session variables are set correctly:
--    SET app.current_instance_id = 'uuid';
--    SET app.current_publisher_id = 'uuid';
--    SET app.current_buyer_id = 'uuid';
--    SET app.user_role = 'admin' | 'publisher' | 'buyer' | 'system';
-- 3. Policies are permissive by default - multiple policies can apply
-- 4. Use RESTRICTIVE policies if you need to explicitly deny access
-- 5. Consolidate multiple policies into single policies when possible (see consolidate_rls_policies migration)
-- 6. After adding policies, verify with:
--    SELECT * FROM pg_policies WHERE tablename = 'table_name';
