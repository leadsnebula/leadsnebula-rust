-- Leads table: Core lead data with encrypted PII
CREATE TABLE IF NOT EXISTS leads (
    uuid UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id VARCHAR(255) NOT NULL UNIQUE,
    fbp_cookie TEXT,
    fbc_cookie TEXT,
    email_sha256 VARCHAR(64),
    phone_sha256 VARCHAR(64),
    tcpa_consent BOOLEAN NOT NULL,
    tcpa_language TEXT NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'processing',
    submitted_at TIMESTAMPTZ,
    sold_at TIMESTAMPTZ,
    buyer_id UUID,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMPTZ,
    ip_address_hash VARCHAR(64),
    user_agent TEXT,
    utm_params JSONB,
    referrer TEXT,
    is_test BOOLEAN NOT NULL DEFAULT false,
    promise_id VARCHAR(64),
    website_url VARCHAR(255),
    publisher_id UUID,
    click_id VARCHAR(255),
    url_consent VARCHAR(255),
    best_call_time VARCHAR(32),
    date_of_birth DATE,
    home_phone VARCHAR(20),
    jornaya_lead_id VARCHAR(255),
    trusted_form_url VARCHAR(255),
    campaign_id UUID,
    strategy VARCHAR(20) NOT NULL,
    request_stage VARCHAR(50),
    ping_id VARCHAR(255),
    post_id VARCHAR(255),
    lead_id VARCHAR(100) UNIQUE,
    session_id VARCHAR(255) NOT NULL,
    vertical_data JSONB NOT NULL DEFAULT '{}',
    request_type VARCHAR(20) NOT NULL,
    vertical_id UUID NOT NULL,
    -- Encrypted PII fields
    first_name_encrypted TEXT,
    last_name_encrypted TEXT,
    email_encrypted TEXT,
    cell_phone_encrypted TEXT,
    street_address_encrypted TEXT,
    city_encrypted TEXT,
    state_encrypted TEXT,
    zip_encrypted TEXT,
    ip_address_encrypted TEXT,
    email_domain VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_request_type CHECK (request_type IN ('ping', 'post', 'fullpost')),
    CONSTRAINT check_status CHECK (status IN ('processing', 'ping_accepted', 'sold', 'rejected', 'timeout', 'invalid', 'error')),
    CONSTRAINT check_strategy CHECK (strategy IN ('pingPost', 'fullPost'))
);

CREATE INDEX idx_leads_event_id ON leads(event_id);
CREATE INDEX idx_leads_lead_id ON leads(lead_id);
CREATE INDEX idx_leads_session_id ON leads(session_id);
CREATE INDEX idx_leads_publisher_id ON leads(publisher_id);
CREATE INDEX idx_leads_campaign_id ON leads(campaign_id);
CREATE INDEX idx_leads_buyer_id ON leads(buyer_id);
CREATE INDEX idx_leads_vertical_id ON leads(vertical_id);
CREATE INDEX idx_leads_status ON leads(status);
CREATE INDEX idx_leads_request_type ON leads(request_type);
CREATE INDEX idx_leads_strategy ON leads(strategy);
CREATE INDEX idx_leads_is_test ON leads(is_test);
CREATE INDEX idx_leads_created_at ON leads(created_at);
CREATE INDEX idx_leads_promise_id ON leads(promise_id) WHERE promise_id IS NOT NULL;
CREATE INDEX idx_leads_ping_id ON leads(ping_id);
CREATE INDEX idx_leads_post_id ON leads(post_id);
CREATE INDEX idx_leads_email_sha256 ON leads(email_sha256);
CREATE INDEX idx_leads_phone_sha256 ON leads(phone_sha256);
CREATE INDEX idx_leads_email_encrypted ON leads(email_encrypted);
CREATE INDEX idx_leads_cell_phone_encrypted ON leads(cell_phone_encrypted);
CREATE INDEX idx_leads_ip_address_encrypted ON leads(ip_address_encrypted);
CREATE INDEX idx_leads_email_domain ON leads(email_domain);
CREATE INDEX idx_leads_request_stage ON leads(request_stage);
CREATE INDEX idx_leads_publisher_status ON leads(publisher_id, status) WHERE is_test = false;
CREATE INDEX idx_leads_publisher_vertical_id_status ON leads(publisher_id, vertical_id, status);
CREATE INDEX idx_leads_campaign_created_at ON leads(campaign_id, created_at DESC);
CREATE INDEX idx_leads_vertical_status_created ON leads(vertical_id, status, created_at DESC);
CREATE INDEX idx_leads_buyer_sold_at ON leads(buyer_id, sold_at DESC) WHERE status = 'sold';
CREATE INDEX idx_leads_prod_created_at ON leads(created_at DESC) WHERE is_test = false;




