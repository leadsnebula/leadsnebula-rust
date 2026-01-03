-- PII access logs table: Tracks all PII access for compliance
CREATE TABLE IF NOT EXISTS pii_access_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lead_id UUID,
    user_id UUID,
    action VARCHAR(100) NOT NULL,
    pii_fields JSONB NOT NULL DEFAULT '[]',
    purpose VARCHAR(255),
    third_party_name VARCHAR(255),
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pii_access_logs_lead_id ON pii_access_logs(lead_id);
CREATE INDEX idx_pii_access_logs_user_id ON pii_access_logs(user_id);
CREATE INDEX idx_pii_access_logs_action ON pii_access_logs(action);
CREATE INDEX idx_pii_access_logs_created_at ON pii_access_logs(created_at DESC);
CREATE INDEX idx_pii_access_logs_third_party_name ON pii_access_logs(third_party_name);




