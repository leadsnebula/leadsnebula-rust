-- Create leads table
CREATE TABLE IF NOT EXISTS leads (
    uuid UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id VARCHAR(255) NOT NULL UNIQUE,
    lead_id VARCHAR(100) UNIQUE,
    publisher_id UUID,
    vertical_id UUID NOT NULL,
    campaign_id UUID,
    buyer_id UUID,
    request_type VARCHAR(20) NOT NULL,
    strategy VARCHAR(20) NOT NULL DEFAULT 'ping_tree',
    status VARCHAR(20) NOT NULL DEFAULT 'processing',
    promise_id VARCHAR(64),
    ping_id VARCHAR(255),
    post_id VARCHAR(255),
    session_id VARCHAR(255),
    request_stage VARCHAR(50),
    
    -- PII (encrypted)
    first_name_encrypted TEXT,
    last_name_encrypted TEXT,
    email_encrypted TEXT,
    cell_phone_encrypted TEXT,
    street_address_encrypted TEXT,
    city_encrypted TEXT,
    state_encrypted TEXT,
    zip_encrypted TEXT,
    ip_address_encrypted TEXT,
    
    -- Hashed fields
    email_sha256 VARCHAR(64),
    phone_sha256 VARCHAR(64),
    ip_address_hash VARCHAR(64),
    email_domain VARCHAR(255),
    
    -- Compliance
    tcpa_consent BOOLEAN NOT NULL DEFAULT false,
    tcpa_language TEXT NOT NULL DEFAULT '',
    
    -- Metadata
    is_test BOOLEAN NOT NULL DEFAULT false,
    user_agent TEXT,
    referrer TEXT,
    website_url VARCHAR(255),
    click_id VARCHAR(255),
    url_consent VARCHAR(255),
    best_call_time VARCHAR(32),
    date_of_birth DATE,
    home_phone VARCHAR(20),
    jornaya_lead_id VARCHAR(255),
    trusted_form_url VARCHAR(255),
    fbp_cookie TEXT,
    fbc_cookie TEXT,
    utm_params JSONB,
    
    -- Timestamps
    submitted_at TIMESTAMP,
    sold_at TIMESTAMP,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMP,
    
    -- Vertical-specific data
    vertical_data JSONB NOT NULL DEFAULT '{}',
    
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_leads_event_id ON leads(event_id);
CREATE INDEX idx_leads_lead_id ON leads(lead_id);
CREATE INDEX idx_leads_publisher_id ON leads(publisher_id);
CREATE INDEX idx_leads_vertical_id ON leads(vertical_id);
CREATE INDEX idx_leads_campaign_id ON leads(campaign_id);
CREATE INDEX idx_leads_buyer_id ON leads(buyer_id);
CREATE INDEX idx_leads_status ON leads(status);
CREATE INDEX idx_leads_request_type ON leads(request_type);
CREATE INDEX idx_leads_promise_id ON leads(promise_id) WHERE promise_id IS NOT NULL;
CREATE INDEX idx_leads_email_sha256 ON leads(email_sha256);
CREATE INDEX idx_leads_phone_sha256 ON leads(phone_sha256);
CREATE INDEX idx_leads_created_at ON leads(created_at);
CREATE INDEX idx_leads_is_test ON leads(is_test);

