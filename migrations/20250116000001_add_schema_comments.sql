-- Add comprehensive table and column comments for schema documentation
-- This migration is idempotent and can be safely re-run

-- ============================================================================
-- PRIORITY TABLES (High Traffic)
-- ============================================================================

-- Leads Table
COMMENT ON TABLE leads IS 'Core lead records for qualification and posting via ping tree. Contains encrypted PII fields (first_name, last_name, email, phone, address). RLS enabled with publisher-level isolation. Status tracked via ENUM (processing, ping_accepted, sold, rejected, timeout, invalid, error, cancelled, expired).';

COMMENT ON COLUMN leads.uuid IS 'Primary key UUID for lead record';
COMMENT ON COLUMN leads.event_id IS 'Unique event identifier for lead submission (unique constraint)';
COMMENT ON COLUMN leads.lead_id IS 'External lead identifier (optional, unique if provided)';
COMMENT ON COLUMN leads.publisher_id IS 'Foreign key to publishers table - identifies lead source';
COMMENT ON COLUMN leads.vertical_id IS 'Foreign key to verticals table - business category (solar, insurance, etc.)';
COMMENT ON COLUMN leads.campaign_id IS 'Foreign key to campaigns table - specific campaign configuration';
COMMENT ON COLUMN leads.buyer_id IS 'Foreign key to buyers table - buyer who purchased the lead (set when sold)';
COMMENT ON COLUMN leads.request_type IS 'Type of request (e.g., "ping", "post", "fullpost")';
COMMENT ON COLUMN leads.strategy IS 'Routing strategy used (default: "ping_tree")';
COMMENT ON COLUMN leads.status IS 'Current processing state - ENUM: processing, ping_accepted, sold, rejected, timeout, invalid, error, cancelled, expired';
COMMENT ON COLUMN leads.promise_id IS 'Promise ID for async operations';
COMMENT ON COLUMN leads.ping_id IS 'Reference to ping record (string ID)';
COMMENT ON COLUMN leads.post_id IS 'Reference to post record (string ID)';
COMMENT ON COLUMN leads.session_id IS 'Session identifier for tracking';
COMMENT ON COLUMN leads.request_stage IS 'Current stage in request processing pipeline';

-- PII Encrypted Fields
COMMENT ON COLUMN leads.first_name_encrypted IS 'Encrypted first name using deterministic encryption for exact-match searches';
COMMENT ON COLUMN leads.last_name_encrypted IS 'Encrypted last name using deterministic encryption';
COMMENT ON COLUMN leads.email_encrypted IS 'Encrypted email address using deterministic encryption for exact-match searches';
COMMENT ON COLUMN leads.cell_phone_encrypted IS 'Encrypted cell phone number';
COMMENT ON COLUMN leads.street_address_encrypted IS 'Encrypted street address';
COMMENT ON COLUMN leads.city_encrypted IS 'Encrypted city name';
COMMENT ON COLUMN leads.state_encrypted IS 'Encrypted state code';
COMMENT ON COLUMN leads.zip_encrypted IS 'Encrypted ZIP code';
COMMENT ON COLUMN leads.ip_address_encrypted IS 'Encrypted IP address';

-- Hashed Fields
COMMENT ON COLUMN leads.email_sha256 IS 'SHA256 hash of email for duplicate detection (indexed)';
COMMENT ON COLUMN leads.phone_sha256 IS 'SHA256 hash of phone for duplicate detection (indexed)';
COMMENT ON COLUMN leads.ip_address_hash IS 'Hash of IP address for analytics';
COMMENT ON COLUMN leads.email_domain IS 'Extracted email domain for filtering/analytics';

-- Compliance
COMMENT ON COLUMN leads.tcpa_consent IS 'TCPA consent flag (required for compliance)';
COMMENT ON COLUMN leads.tcpa_language IS 'TCPA consent language text';

-- Metadata
COMMENT ON COLUMN leads.is_test IS 'Flag indicating test lead (excluded from production processing)';
COMMENT ON COLUMN leads.user_agent IS 'HTTP user agent string';
COMMENT ON COLUMN leads.referrer IS 'HTTP referrer URL';
COMMENT ON COLUMN leads.website_url IS 'Source website URL';
COMMENT ON COLUMN leads.click_id IS 'Click tracking identifier';
COMMENT ON COLUMN leads.url_consent IS 'Consent URL';
COMMENT ON COLUMN leads.best_call_time IS 'Preferred call time window';
COMMENT ON COLUMN leads.date_of_birth IS 'Date of birth (if provided)';
COMMENT ON COLUMN leads.home_phone IS 'Home phone number (unencrypted, optional)';
COMMENT ON COLUMN leads.jornaya_lead_id IS 'Jornaya lead identifier';
COMMENT ON COLUMN leads.trusted_form_url IS 'TrustedForm verification URL';
COMMENT ON COLUMN leads.fbp_cookie IS 'Facebook Pixel cookie data';
COMMENT ON COLUMN leads.fbc_cookie IS 'Facebook Click cookie data';
COMMENT ON COLUMN leads.utm_params IS 'UTM parameters as JSONB for marketing attribution';

-- Timestamps
COMMENT ON COLUMN leads.submitted_at IS 'Timestamp when lead was originally submitted';
COMMENT ON COLUMN leads.sold_at IS 'Timestamp when lead was sold to buyer';
COMMENT ON COLUMN leads.retry_count IS 'Number of retry attempts for failed operations';
COMMENT ON COLUMN leads.next_retry_at IS 'Scheduled timestamp for next retry';
COMMENT ON COLUMN leads.vertical_data IS 'Vertical-specific data as JSONB (flexible schema per vertical)';
COMMENT ON COLUMN leads.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN leads.updated_at IS 'Record last update timestamp';

-- Buyers Table
COMMENT ON TABLE buyers IS 'Buyer entities that purchase leads. Contains business contact information (not personal PII). RLS enabled with instance-level isolation. Status tracked via ENUM (incomplete, active, paused, suspended).';

COMMENT ON COLUMN buyers.id IS 'Primary key UUID';
COMMENT ON COLUMN buyers.name IS 'Buyer business name';
COMMENT ON COLUMN buyers.instance_id IS 'Foreign key to instances table - tenant isolation';
COMMENT ON COLUMN buyers.instance_user_id IS 'Foreign key to instance_users table - user who created buyer';
COMMENT ON COLUMN buyers.vertical_id IS 'Foreign key to verticals table - primary vertical category';
COMMENT ON COLUMN buyers.buyer_integration_id IS 'Foreign key to buyer_integrations table - integration template';
COMMENT ON COLUMN buyers.status IS 'Buyer status - ENUM: incomplete, active, paused, suspended';
COMMENT ON COLUMN buyers.deleted_at IS 'Soft delete timestamp (NULL if active)';
COMMENT ON COLUMN buyers.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN buyers.updated_at IS 'Record last update timestamp';

-- Publishers Table
COMMENT ON TABLE publishers IS 'Publisher entities that submit leads. RLS enabled with instance-level isolation. Status tracked via ENUM (active, paused, suspended).';

COMMENT ON COLUMN publishers.id IS 'Primary key UUID';
COMMENT ON COLUMN publishers.name IS 'Publisher business name';
COMMENT ON COLUMN publishers.email IS 'Publisher contact email (unique)';
COMMENT ON COLUMN publishers.api_key_hash IS 'SHA256 hash of API key for authentication (unique, indexed)';
COMMENT ON COLUMN publishers.api_key_prefix IS 'First 20 characters of API key for identification';
COMMENT ON COLUMN publishers.status IS 'Publisher status - ENUM: active, paused, suspended';
COMMENT ON COLUMN publishers.total_requests IS 'Total number of API requests made';
COMMENT ON COLUMN publishers.last_request_at IS 'Timestamp of last API request';
COMMENT ON COLUMN publishers.instance_id IS 'Foreign key to instances table - tenant isolation';
COMMENT ON COLUMN publishers.instance_user_id IS 'Foreign key to instance_users table - user who created publisher';
COMMENT ON COLUMN publishers.is_documentation_test IS 'Flag for documentation/testing purposes';
COMMENT ON COLUMN publishers.hmac_secret_hash IS 'SHA256 hash of HMAC secret for request signing';
COMMENT ON COLUMN publishers.hmac_secret_prefix IS 'First 20 characters of HMAC secret';
COMMENT ON COLUMN publishers.hmac_required IS 'Flag indicating HMAC signing is required';
COMMENT ON COLUMN publishers.hmac_secret_encrypted IS 'Encrypted HMAC secret';
COMMENT ON COLUMN publishers.deleted_at IS 'Soft delete timestamp (NULL if active)';
COMMENT ON COLUMN publishers.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN publishers.updated_at IS 'Record last update timestamp';

-- Campaigns Table
COMMENT ON TABLE campaigns IS 'Campaign configurations linking buyers to publishers for specific verticals. RLS enabled with instance-level isolation. Status tracked via ENUM (active, paused, suspended).';

COMMENT ON COLUMN campaigns.id IS 'Primary key UUID';
COMMENT ON COLUMN campaigns.buyer_id IS 'Foreign key to buyers table';
COMMENT ON COLUMN campaigns.publisher_id IS 'Foreign key to publishers table';
COMMENT ON COLUMN campaigns.instance_id IS 'Foreign key to instances table - tenant isolation';
COMMENT ON COLUMN campaigns.name IS 'Campaign name (optional)';
COMMENT ON COLUMN campaigns.vertical IS 'Vertical category (e.g., "solar", "insurance")';
COMMENT ON COLUMN campaigns.campaign_token IS 'Unique campaign token for API identification';
COMMENT ON COLUMN campaigns.status IS 'Campaign status - ENUM: active, paused, suspended';
COMMENT ON COLUMN campaigns.is_documentation_test IS 'Flag for documentation/testing purposes';
COMMENT ON COLUMN campaigns.deleted_at IS 'Soft delete timestamp (NULL if active)';
COMMENT ON COLUMN campaigns.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN campaigns.updated_at IS 'Record last update timestamp';

-- Audit Logs Table
COMMENT ON TABLE audit_logs IS 'Comprehensive audit trail for all platform actions. RLS enabled with instance-level isolation. Tracks user actions, resource changes, IP addresses, and user agents for compliance and debugging.';

COMMENT ON COLUMN audit_logs.id IS 'Primary key UUID';
COMMENT ON COLUMN audit_logs.instance_id IS 'Foreign key to instances table - tenant isolation';
COMMENT ON COLUMN audit_logs.instance_user_id IS 'Foreign key to instance_users table - user who performed action';
COMMENT ON COLUMN audit_logs.action_type IS 'Type of action performed (e.g., "create", "update", "delete", "login")';
COMMENT ON COLUMN audit_logs.resource_type IS 'Type of resource affected (e.g., "lead", "buyer", "publisher")';
COMMENT ON COLUMN audit_logs.resource_id IS 'UUID of affected resource';
COMMENT ON COLUMN audit_logs.details IS 'Action details as JSONB (flexible structure)';
COMMENT ON COLUMN audit_logs.affected_resources IS 'Array of affected resources as JSONB (for bulk operations)';
COMMENT ON COLUMN audit_logs.ip_address IS 'IP address of request origin';
COMMENT ON COLUMN audit_logs.user_agent IS 'HTTP user agent string';
COMMENT ON COLUMN audit_logs.created_at IS 'Action timestamp';
COMMENT ON COLUMN audit_logs.updated_at IS 'Record last update timestamp';

-- Ping Trees Table
COMMENT ON TABLE ping_trees IS 'Ping tree routing configurations defining buyer priority and routing logic per vertical. RLS enabled with instance-level isolation. Strategy: ping_post (ping then post) or fullpost (direct post). Publishers are linked via ping_tree_publishers join table.';

COMMENT ON COLUMN ping_trees.id IS 'Primary key UUID';
COMMENT ON COLUMN ping_trees.instance_id IS 'Foreign key to instances table - tenant isolation';
COMMENT ON COLUMN ping_trees.name IS 'Ping tree name (unique per instance/vertical)';
COMMENT ON COLUMN ping_trees.vertical IS 'Vertical category (e.g., "solar", "insurance")';
COMMENT ON COLUMN ping_trees.strategy IS 'Routing strategy - ENUM: ping_post, fullpost';
COMMENT ON COLUMN ping_trees.status IS 'Ping tree status - ENUM: active, paused';
COMMENT ON COLUMN ping_trees.priority IS 'Priority ordering (lower number = higher priority)';
COMMENT ON COLUMN ping_trees.deleted_at IS 'Soft delete timestamp (NULL if active)';
COMMENT ON COLUMN ping_trees.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN ping_trees.updated_at IS 'Record last update timestamp';

-- Ping Tree Campaigns Table
COMMENT ON TABLE ping_tree_campaigns IS 'Junction table linking ping trees to campaigns with priority and pricing. Defines the order and conditions for buyer routing within a ping tree.';

COMMENT ON COLUMN ping_tree_campaigns.id IS 'Primary key UUID';
COMMENT ON COLUMN ping_tree_campaigns.ping_tree_id IS 'Foreign key to ping_trees table';
COMMENT ON COLUMN ping_tree_campaigns.campaign_id IS 'Foreign key to campaigns table';
COMMENT ON COLUMN ping_tree_campaigns.priority IS 'Priority within ping tree (lower number = higher priority)';
COMMENT ON COLUMN ping_tree_campaigns.min_price IS 'Minimum acceptable price for this campaign';
COMMENT ON COLUMN ping_tree_campaigns.max_price IS 'Maximum acceptable price for this campaign';
COMMENT ON COLUMN ping_tree_campaigns.enabled IS 'Flag to enable/disable this campaign in routing';
COMMENT ON COLUMN ping_tree_campaigns.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN ping_tree_campaigns.updated_at IS 'Record last update timestamp';

-- Pulsar Decision Logs Table
COMMENT ON TABLE pulsar_decision_logs IS 'Decision logging for qualification engine (Pulsar). Records buyer acceptance/rejection decisions with rule evaluations and bid prices. Used for debugging and analytics.';

COMMENT ON COLUMN pulsar_decision_logs.id IS 'Primary key UUID';
COMMENT ON COLUMN pulsar_decision_logs.lead_id IS 'Foreign key to leads table (UUID)';
COMMENT ON COLUMN pulsar_decision_logs.ping_id IS 'Ping identifier (string)';
COMMENT ON COLUMN pulsar_decision_logs.buyer_id IS 'Foreign key to buyers table';
COMMENT ON COLUMN pulsar_decision_logs.accepted IS 'Boolean flag: true if buyer accepted lead';
COMMENT ON COLUMN pulsar_decision_logs.final_bid_price IS 'Final bid price offered by buyer';
COMMENT ON COLUMN pulsar_decision_logs.rule_evaluations IS 'Rule evaluation results as JSONB';
COMMENT ON COLUMN pulsar_decision_logs.evaluated_at IS 'Timestamp when decision was made';
COMMENT ON COLUMN pulsar_decision_logs.created_at IS 'Record creation timestamp';

-- ============================================================================
-- SECONDARY TABLES
-- ============================================================================

-- Instances Table
COMMENT ON TABLE instances IS 'Top-level tenant isolation. Each instance represents a separate customer/organization with isolated data. RLS enabled.';

COMMENT ON COLUMN instances.id IS 'Primary key UUID';
COMMENT ON COLUMN instances.name IS 'Instance/organization name';
COMMENT ON COLUMN instances.instance_user_id IS 'Foreign key to instance_users table - primary admin user';
COMMENT ON COLUMN instances.payment_status IS 'Payment status - ENUM: trial, active, past_due, suspended';
COMMENT ON COLUMN instances.subscription_tier IS 'Subscription tier identifier';
COMMENT ON COLUMN instances.trial_ends_at IS 'Trial expiration timestamp';
COMMENT ON COLUMN instances.max_publishers IS 'Maximum allowed publishers (999999 = unlimited)';
COMMENT ON COLUMN instances.max_buyers IS 'Maximum allowed buyers (999999 = unlimited)';
COMMENT ON COLUMN instances.max_campaigns IS 'Maximum allowed campaigns (999999 = unlimited)';
COMMENT ON COLUMN instances.max_leads IS 'Maximum allowed leads (999999 = unlimited)';
COMMENT ON COLUMN instances.max_requests_per_hour IS 'Rate limit for API requests (999999 = unlimited)';
COMMENT ON COLUMN instances.deleted_at IS 'Soft delete timestamp (NULL if active)';
COMMENT ON COLUMN instances.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN instances.updated_at IS 'Record last update timestamp';

-- Instance Users Table
COMMENT ON TABLE instance_users IS 'Platform-level user accounts with authentication. Supports email/password, OTP, and WebAuthn. RLS enabled with instance-level isolation.';

COMMENT ON COLUMN instance_users.id IS 'Primary key UUID';
COMMENT ON COLUMN instance_users.sequential_id IS 'Sequential integer ID for user-friendly display';
COMMENT ON COLUMN instance_users.email IS 'User email address (unique, indexed)';
COMMENT ON COLUMN instance_users.encrypted_password IS 'Encrypted password (bcrypt hash)';
COMMENT ON COLUMN instance_users.confirmation_token IS 'Email confirmation token (unique if set)';
COMMENT ON COLUMN instance_users.confirmed_at IS 'Email confirmation timestamp';
COMMENT ON COLUMN instance_users.confirmation_sent_at IS 'Confirmation email sent timestamp';
COMMENT ON COLUMN instance_users.reset_password_token IS 'Password reset token (unique if set)';
COMMENT ON COLUMN instance_users.reset_password_sent_at IS 'Password reset email sent timestamp';
COMMENT ON COLUMN instance_users.remember_created_at IS 'Remember me cookie creation timestamp';
COMMENT ON COLUMN instance_users.failed_attempts IS 'Number of failed login attempts';
COMMENT ON COLUMN instance_users.unlock_token IS 'Account unlock token (unique if set)';
COMMENT ON COLUMN instance_users.locked_at IS 'Account lock timestamp';
COMMENT ON COLUMN instance_users.sign_in_count IS 'Total number of sign-ins';
COMMENT ON COLUMN instance_users.current_sign_in_at IS 'Current session sign-in timestamp';
COMMENT ON COLUMN instance_users.last_sign_in_at IS 'Previous session sign-in timestamp';
COMMENT ON COLUMN instance_users.current_sign_in_ip IS 'Current session IP address';
COMMENT ON COLUMN instance_users.last_sign_in_ip IS 'Previous session IP address';
COMMENT ON COLUMN instance_users.first_name IS 'User first name';
COMMENT ON COLUMN instance_users.last_name IS 'User last name';
COMMENT ON COLUMN instance_users.phone IS 'User phone number';
COMMENT ON COLUMN instance_users.phone_verified_at IS 'Phone verification timestamp';
COMMENT ON COLUMN instance_users.timezone IS 'User timezone preference (default: Pacific Time)';
COMMENT ON COLUMN instance_users.locale IS 'User locale preference (default: en)';
COMMENT ON COLUMN instance_users.last_password_change_at IS 'Last password change timestamp';
COMMENT ON COLUMN instance_users.status IS 'User status - ENUM: active, suspended, revoked, pending_verification';
COMMENT ON COLUMN instance_users.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN instance_users.updated_at IS 'Record last update timestamp';
COMMENT ON COLUMN instance_users.unconfirmed_email IS 'Unconfirmed email address (during email change)';
COMMENT ON COLUMN instance_users.business_name IS 'User business name';
COMMENT ON COLUMN instance_users.preferred_2fa_method IS 'Preferred 2FA method (default: otp)';
COMMENT ON COLUMN instance_users.passwordless_login_enabled IS 'Flag for passwordless login (WebAuthn)';

-- Verticals Table
COMMENT ON TABLE verticals IS 'Business vertical categories (e.g., solar, insurance, home services). Defines the type of leads and buyers.';

COMMENT ON COLUMN verticals.id IS 'Primary key UUID';
COMMENT ON COLUMN verticals.name IS 'Vertical name (e.g., "Solar", "Insurance")';
COMMENT ON COLUMN verticals.slug IS 'URL-friendly identifier (unique)';
COMMENT ON COLUMN verticals.is_active IS 'Flag indicating if vertical is active';
COMMENT ON COLUMN verticals.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN verticals.updated_at IS 'Record last update timestamp';

-- Buyer Integrations Table
COMMENT ON TABLE buyer_integrations IS 'Buyer integration templates defining API endpoints, configuration, and posting URLs. Used as templates for buyer setup.';

COMMENT ON COLUMN buyer_integrations.id IS 'Primary key UUID';
COMMENT ON COLUMN buyer_integrations.name IS 'Integration name';
COMMENT ON COLUMN buyer_integrations.slug IS 'URL-friendly identifier (unique)';
COMMENT ON COLUMN buyer_integrations.vertical_id IS 'Foreign key to verticals table';
COMMENT ON COLUMN buyer_integrations.description IS 'Integration description';
COMMENT ON COLUMN buyer_integrations.configuration_template IS 'Configuration template as JSONB';
COMMENT ON COLUMN buyer_integrations.default_timeout IS 'Default timeout in seconds (default: 1.2)';
COMMENT ON COLUMN buyer_integrations.posting_url_template IS 'URL template for posting leads';
COMMENT ON COLUMN buyer_integrations.is_internal IS 'Flag for internal-only integrations';
COMMENT ON COLUMN buyer_integrations.status IS 'Integration status - ENUM: available, deprecated, hidden';
COMMENT ON COLUMN buyer_integrations.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN buyer_integrations.updated_at IS 'Record last update timestamp';

-- Buyer Qualification Configs Table
COMMENT ON TABLE buyer_qualification_configs IS 'Per-buyer qualification rules and configurations. Defines which leads a buyer will accept based on rules, pricing, and conditions.';

COMMENT ON COLUMN buyer_qualification_configs.id IS 'Primary key UUID';
COMMENT ON COLUMN buyer_qualification_configs.buyer_id IS 'Foreign key to buyers table';
COMMENT ON COLUMN buyer_qualification_configs.vertical_id IS 'Foreign key to verticals table';
COMMENT ON COLUMN buyer_qualification_configs.buyer_integration_id IS 'Foreign key to buyer_integrations table';
COMMENT ON COLUMN buyer_qualification_configs.rule_set_name IS 'Name of rule set (unique per buyer/vertical)';
COMMENT ON COLUMN buyer_qualification_configs.config IS 'Configuration as JSONB (rules, pricing, conditions)';
COMMENT ON COLUMN buyer_qualification_configs.rules_order IS 'Array of rule names defining evaluation order';
COMMENT ON COLUMN buyer_qualification_configs.enabled IS 'Flag to enable/disable this config';
COMMENT ON COLUMN buyer_qualification_configs.is_active IS 'Flag for active/inactive status';
COMMENT ON COLUMN buyer_qualification_configs.timeout_seconds IS 'Timeout in seconds for qualification requests';
COMMENT ON COLUMN buyer_qualification_configs.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN buyer_qualification_configs.updated_at IS 'Record last update timestamp';

-- Buyer Zip Lists Table
COMMENT ON TABLE buyer_zip_lists IS 'ZIP code blacklists/whitelists for buyers. Used to filter leads by geographic location with optional price adjustments.';

COMMENT ON COLUMN buyer_zip_lists.id IS 'Primary key UUID';
COMMENT ON COLUMN buyer_zip_lists.buyer_id IS 'Foreign key to buyers table';
COMMENT ON COLUMN buyer_zip_lists.name IS 'List name';
COMMENT ON COLUMN buyer_zip_lists.list_type IS 'List type - ENUM: blacklist, whitelist';
COMMENT ON COLUMN buyer_zip_lists.price_adjustment IS 'Price adjustment amount (can be negative)';
COMMENT ON COLUMN buyer_zip_lists.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN buyer_zip_lists.updated_at IS 'Record last update timestamp';

-- Buyer Zip Codes Table
COMMENT ON TABLE buyer_zip_codes IS 'Individual ZIP codes within buyer zip lists. Links ZIP codes to lists for geographic filtering.';

COMMENT ON COLUMN buyer_zip_codes.id IS 'Primary key UUID';
COMMENT ON COLUMN buyer_zip_codes.buyer_zip_list_id IS 'Foreign key to buyer_zip_lists table';
COMMENT ON COLUMN buyer_zip_codes.zip IS '5-digit ZIP code';
COMMENT ON COLUMN buyer_zip_codes.created_at IS 'Record creation timestamp';
COMMENT ON COLUMN buyer_zip_codes.updated_at IS 'Record last update timestamp';

-- ============================================================================
-- NOTE: Tables managed in Ruby migrations (pings, posts, lead_sales, etc.)
-- may have separate documentation. Check Ruby migration files for details.
-- ============================================================================
