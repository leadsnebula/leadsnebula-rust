-- Ping payloads table: Encrypted ping request/response payloads
CREATE TABLE IF NOT EXISTS ping_payloads (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ping_id BIGINT NOT NULL,
    lead_id UUID NOT NULL,
    request_payload_encrypted TEXT NOT NULL,
    response_payload_encrypted TEXT,
    buyer_endpoint VARCHAR(255),
    http_status_code INTEGER,
    response_status VARCHAR(50),
    bid_amount DECIMAL(10, 2),
    redaction_scheduled_at TIMESTAMPTZ,
    is_redacted BOOLEAN NOT NULL DEFAULT false,
    redacted_at TIMESTAMPTZ,
    last_accessed_at TIMESTAMPTZ,
    last_accessed_by VARCHAR(100),
    access_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ping_payloads_ping_id ON ping_payloads(ping_id);
CREATE INDEX idx_ping_payloads_lead_id ON ping_payloads(lead_id);
CREATE INDEX idx_ping_payloads_response_status ON ping_payloads(response_status);
CREATE INDEX idx_ping_payloads_is_redacted ON ping_payloads(is_redacted);
CREATE INDEX idx_ping_payloads_redaction_scheduled_at ON ping_payloads(redaction_scheduled_at);




