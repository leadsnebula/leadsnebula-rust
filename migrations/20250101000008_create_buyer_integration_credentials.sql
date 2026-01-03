-- Buyer integration credentials table: Encrypted API credentials for buyers
CREATE TABLE IF NOT EXISTS buyer_integration_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_id UUID NOT NULL,
    buyer_integration_id UUID NOT NULL,
    vertical_id UUID,
    encrypted_api_key TEXT NOT NULL,
    encrypted_secret TEXT,
    other_encrypted_fields JSONB NOT NULL DEFAULT '{}',
    status VARCHAR(50) NOT NULL DEFAULT 'enabled',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT idx_buyer_integration_credentials_unique UNIQUE (buyer_id, buyer_integration_id)
);

CREATE INDEX idx_buyer_integration_credentials_buyer_id ON buyer_integration_credentials(buyer_id);
CREATE INDEX idx_buyer_integration_credentials_buyer_integration_id ON buyer_integration_credentials(buyer_integration_id);
CREATE INDEX idx_buyer_integration_credentials_vertical_id ON buyer_integration_credentials(vertical_id);
CREATE INDEX idx_buyer_integration_credentials_status ON buyer_integration_credentials(status);




