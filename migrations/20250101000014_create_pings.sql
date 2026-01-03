-- Pings table: Ping requests to buyers
CREATE TABLE IF NOT EXISTS pings (
    id BIGSERIAL PRIMARY KEY,
    ping_id TEXT NOT NULL UNIQUE,
    promise_id TEXT,
    state TEXT,
    result TEXT,
    buyer_response JSONB,
    error_message TEXT,
    response_time_ms INTEGER,
    sent_at TIMESTAMPTZ,
    lead_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pings_ping_id ON pings(ping_id);
CREATE INDEX idx_pings_promise_id ON pings(promise_id);
CREATE INDEX idx_pings_lead_id ON pings(lead_id);
CREATE INDEX idx_pings_result ON pings(result);
CREATE INDEX idx_pings_created_at ON pings(created_at DESC);
CREATE INDEX idx_pings_lead_created ON pings(lead_id, created_at DESC);
CREATE INDEX idx_pings_buyer_response_gin ON pings USING GIN (buyer_response);




