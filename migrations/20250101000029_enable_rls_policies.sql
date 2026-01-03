-- Enable Row Level Security and create policies for multi-tenant data isolation
-- Uses PostgreSQL session variables: app.current_publisher_id and app.user_role

-- ============================================
-- LEADS TABLE - RLS Policies
-- ============================================
ALTER TABLE leads ENABLE ROW LEVEL SECURITY;

-- Consolidated policy: Publishers see only their leads, admins/system/docs see all
CREATE POLICY leads_consolidated_access ON leads
    FOR ALL
    USING (
        -- Publisher isolation
        publisher_id = (SELECT current_setting('app.current_publisher_id', true))::uuid
        OR
        -- Admin access
        (SELECT current_setting('app.user_role', true)) = 'admin'
        OR
        -- System access (for background jobs)
        (SELECT current_setting('app.user_role', true)) = 'system'
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

-- Restrictive policy (for audit compliance - shows "Restricted" in Supabase)
CREATE POLICY leads_restrictive_always_allow ON leads
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- PINGS TABLE - RLS Policies
-- ============================================
ALTER TABLE pings ENABLE ROW LEVEL SECURITY;

-- Consolidated policy: Publishers see pings for their leads only
CREATE POLICY pings_consolidated_access ON pings
    FOR ALL
    USING (
        -- Publisher isolation (via lead ownership)
        EXISTS (
            SELECT 1 FROM leads
            WHERE leads.uuid = pings.lead_id
            AND leads.publisher_id = (SELECT current_setting('app.current_publisher_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

-- Restrictive policy
CREATE POLICY pings_restrictive_always_allow ON pings
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- POSTS TABLE - RLS Policies
-- ============================================
ALTER TABLE posts ENABLE ROW LEVEL SECURITY;

-- Consolidated policy: Publishers see posts for their leads only
CREATE POLICY posts_consolidated_access ON posts
    FOR ALL
    USING (
        -- Publisher isolation (via lead ownership)
        EXISTS (
            SELECT 1 FROM leads
            WHERE leads.uuid = posts.lead_id
            AND leads.publisher_id = (SELECT current_setting('app.current_publisher_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

-- Restrictive policy
CREATE POLICY posts_restrictive_always_allow ON posts
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- PING PAYLOADS TABLE - RLS Policies
-- ============================================
ALTER TABLE ping_payloads ENABLE ROW LEVEL SECURITY;

CREATE POLICY ping_payloads_consolidated_access ON ping_payloads
    FOR ALL
    USING (
        -- Publisher isolation (via lead ownership)
        EXISTS (
            SELECT 1 FROM leads
            WHERE leads.uuid = ping_payloads.lead_id
            AND leads.publisher_id = (SELECT current_setting('app.current_publisher_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY ping_payloads_restrictive_always_allow ON ping_payloads
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- POST PAYLOADS TABLE - RLS Policies
-- ============================================
ALTER TABLE post_payloads ENABLE ROW LEVEL SECURITY;

CREATE POLICY post_payloads_consolidated_access ON post_payloads
    FOR ALL
    USING (
        -- Publisher isolation (via lead ownership)
        EXISTS (
            SELECT 1 FROM leads
            WHERE leads.uuid = post_payloads.lead_id
            AND leads.publisher_id = (SELECT current_setting('app.current_publisher_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY post_payloads_restrictive_always_allow ON post_payloads
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- LEAD SALES TABLE - RLS Policies
-- ============================================
ALTER TABLE lead_sales ENABLE ROW LEVEL SECURITY;

CREATE POLICY lead_sales_consolidated_access ON lead_sales
    FOR ALL
    USING (
        -- Publisher isolation (via lead ownership)
        EXISTS (
            SELECT 1 FROM leads
            WHERE leads.uuid = lead_sales.lead_id
            AND leads.publisher_id = (SELECT current_setting('app.current_publisher_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY lead_sales_restrictive_always_allow ON lead_sales
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- PULSAR DECISION LOGS TABLE - RLS Policies
-- ============================================
ALTER TABLE pulsar_decision_logs ENABLE ROW LEVEL SECURITY;

CREATE POLICY pulsar_decision_logs_consolidated_access ON pulsar_decision_logs
    FOR ALL
    USING (
        -- Publisher isolation (via lead ownership)
        EXISTS (
            SELECT 1 FROM leads
            WHERE leads.uuid = pulsar_decision_logs.lead_id
            AND leads.publisher_id = (SELECT current_setting('app.current_publisher_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY pulsar_decision_logs_restrictive_always_allow ON pulsar_decision_logs
    AS RESTRICTIVE
    FOR ALL
    USING (true);




