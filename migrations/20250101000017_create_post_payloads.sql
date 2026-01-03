-- Post payloads table: Encrypted post request/response payloads
CREATE TABLE IF NOT EXISTS post_payloads (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lead_id UUID NOT NULL,
    request_payload_encrypted TEXT NOT NULL,
    response_payload_encrypted TEXT,
    buyer_endpoint VARCHAR(255),
    http_status_code INTEGER,
    response_status VARCHAR(50),
    sale_price DECIMAL(10, 2),
    price DECIMAL(10, 2),
    post_id TEXT UNIQUE,
    redaction_scheduled_at TIMESTAMPTZ,
    is_redacted BOOLEAN NOT NULL DEFAULT false,
    redacted_at TIMESTAMPTZ,
    last_accessed_at TIMESTAMPTZ,
    last_accessed_by VARCHAR(100),
    access_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_post_payloads_post_id ON post_payloads(post_id);
CREATE INDEX idx_post_payloads_lead_id ON post_payloads(lead_id);
CREATE INDEX idx_post_payloads_response_status ON post_payloads(response_status);
CREATE INDEX idx_post_payloads_price ON post_payloads(price);
CREATE INDEX idx_post_payloads_is_redacted ON post_payloads(is_redacted);
CREATE INDEX idx_post_payloads_redaction_scheduled_at ON post_payloads(redaction_scheduled_at);




