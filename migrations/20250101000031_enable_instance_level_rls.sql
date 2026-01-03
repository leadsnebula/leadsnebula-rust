-- Enable Row Level Security for instance-level tables
-- Uses PostgreSQL session variable: app.current_instance_id

-- ============================================
-- PUBLISHERS TABLE - Instance-level RLS
-- ============================================
ALTER TABLE publishers ENABLE ROW LEVEL SECURITY;

CREATE POLICY publishers_instance_isolation ON publishers
    FOR ALL
    USING (
        -- Instance isolation
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY publishers_restrictive_always_allow ON publishers
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- BUYERS TABLE - Instance-level RLS
-- ============================================
ALTER TABLE buyers ENABLE ROW LEVEL SECURITY;

CREATE POLICY buyers_instance_isolation ON buyers
    FOR ALL
    USING (
        -- Instance isolation
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY buyers_restrictive_always_allow ON buyers
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- CAMPAIGNS TABLE - Instance-level RLS
-- ============================================
ALTER TABLE campaigns ENABLE ROW LEVEL SECURITY;

CREATE POLICY campaigns_instance_isolation ON campaigns
    FOR ALL
    USING (
        -- Instance isolation
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY campaigns_restrictive_always_allow ON campaigns
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- PING_TREES TABLE - Instance-level RLS
-- ============================================
ALTER TABLE ping_trees ENABLE ROW LEVEL SECURITY;

CREATE POLICY ping_trees_instance_isolation ON ping_trees
    FOR ALL
    USING (
        -- Instance isolation
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY ping_trees_restrictive_always_allow ON ping_trees
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- BUYER_INTEGRATION_CREDENTIALS TABLE - Instance-level RLS (via buyer)
-- ============================================
ALTER TABLE buyer_integration_credentials ENABLE ROW LEVEL SECURITY;

CREATE POLICY buyer_integration_credentials_instance_isolation ON buyer_integration_credentials
    FOR ALL
    USING (
        -- Instance isolation (via buyer)
        EXISTS (
            SELECT 1 FROM buyers
            WHERE buyers.id = buyer_integration_credentials.buyer_id
            AND buyers.instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY buyer_integration_credentials_restrictive_always_allow ON buyer_integration_credentials
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- BUYER_QUALIFICATION_CONFIGS TABLE - Instance-level RLS (via buyer)
-- ============================================
ALTER TABLE buyer_qualification_configs ENABLE ROW LEVEL SECURITY;

CREATE POLICY buyer_qualification_configs_instance_isolation ON buyer_qualification_configs
    FOR ALL
    USING (
        -- Instance isolation (via buyer)
        EXISTS (
            SELECT 1 FROM buyers
            WHERE buyers.id = buyer_qualification_configs.buyer_id
            AND buyers.instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY buyer_qualification_configs_restrictive_always_allow ON buyer_qualification_configs
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- AUDIT_LOGS TABLE - Instance-level RLS
-- ============================================
ALTER TABLE audit_logs ENABLE ROW LEVEL SECURITY;

CREATE POLICY audit_logs_instance_isolation ON audit_logs
    FOR ALL
    USING (
        -- Instance isolation
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY audit_logs_restrictive_always_allow ON audit_logs
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- PII_ACCESS_LOGS TABLE - Instance-level RLS (via lead -> publisher -> instance)
-- ============================================
ALTER TABLE pii_access_logs ENABLE ROW LEVEL SECURITY;

CREATE POLICY pii_access_logs_instance_isolation ON pii_access_logs
    FOR ALL
    USING (
        -- Instance isolation (via lead -> publisher -> instance)
        EXISTS (
            SELECT 1 FROM leads l
            JOIN publishers p ON p.id = l.publisher_id
            WHERE l.uuid = pii_access_logs.lead_id
            AND p.instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        )
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY pii_access_logs_restrictive_always_allow ON pii_access_logs
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- PASSWORD_POLICY_CONFIG TABLE - Instance-level RLS
-- ============================================
ALTER TABLE password_policy_config ENABLE ROW LEVEL SECURITY;

CREATE POLICY password_policy_config_instance_isolation ON password_policy_config
    FOR ALL
    USING (
        -- Instance isolation
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY password_policy_config_restrictive_always_allow ON password_policy_config
    AS RESTRICTIVE
    FOR ALL
    USING (true);

-- ============================================
-- DATA_PROCESSING_AGREEMENTS TABLE - Instance-level RLS
-- ============================================
ALTER TABLE data_processing_agreements ENABLE ROW LEVEL SECURITY;

CREATE POLICY data_processing_agreements_instance_isolation ON data_processing_agreements
    FOR ALL
    USING (
        -- Instance isolation
        instance_id = (SELECT current_setting('app.current_instance_id', true))::uuid
        OR
        -- Admin/system access
        (SELECT current_setting('app.user_role', true)) IN ('admin', 'system')
        OR
        -- Documentation test access
        (SELECT current_setting('app.user_role', true)) = 'documentation_test'
    );

CREATE POLICY data_processing_agreements_restrictive_always_allow ON data_processing_agreements
    AS RESTRICTIVE
    FOR ALL
    USING (true);

