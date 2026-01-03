-- Buyer integrations table: Integration templates/configs
CREATE TABLE IF NOT EXISTS buyer_integrations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL UNIQUE,
    vertical_id UUID NOT NULL,
    description TEXT,
    configuration_template JSONB NOT NULL DEFAULT '{}',
    default_timeout DECIMAL(5, 2) NOT NULL DEFAULT 1.2,
    posting_url_template TEXT,
    is_internal BOOLEAN NOT NULL DEFAULT false,
    status VARCHAR(50) NOT NULL DEFAULT 'available',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_buyer_integrations_slug ON buyer_integrations(slug);
CREATE INDEX idx_buyer_integrations_vertical_id ON buyer_integrations(vertical_id);
CREATE INDEX idx_buyer_integrations_status ON buyer_integrations(status);
CREATE INDEX idx_buyer_integrations_vertical_status ON buyer_integrations(vertical_id, status) WHERE status = 'available';




