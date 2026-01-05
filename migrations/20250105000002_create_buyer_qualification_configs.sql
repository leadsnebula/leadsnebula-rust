-- Create buyer_integrations table first (if it doesn't exist)
CREATE TABLE IF NOT EXISTS buyer_integrations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL UNIQUE,
    vertical_id UUID NOT NULL,
    description TEXT,
    configuration_template JSONB DEFAULT '{}',
    default_timeout DECIMAL(5, 2) DEFAULT 1.2,
    posting_url_template TEXT,
    is_internal BOOLEAN DEFAULT false NOT NULL,
    status VARCHAR(20) DEFAULT 'available' NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_buyer_integrations_vertical FOREIGN KEY (vertical_id) REFERENCES verticals(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_buyer_integrations_slug ON buyer_integrations(slug);
CREATE INDEX IF NOT EXISTS idx_buyer_integrations_vertical_id ON buyer_integrations(vertical_id);
CREATE INDEX IF NOT EXISTS idx_buyer_integrations_status ON buyer_integrations(status);
CREATE INDEX IF NOT EXISTS idx_buyer_integrations_vertical_status ON buyer_integrations(vertical_id, status) WHERE status = 'available';

-- Create buyer_qualification_configs table
CREATE TABLE IF NOT EXISTS buyer_qualification_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_id UUID NOT NULL,
    vertical_id UUID NOT NULL,
    buyer_integration_id UUID,
    rule_set_name VARCHAR(255) NOT NULL,
    config JSONB DEFAULT '{}' NOT NULL,
    rules_order VARCHAR(50)[] DEFAULT ARRAY[]::VARCHAR(50)[],
    enabled BOOLEAN DEFAULT true NOT NULL,
    is_active BOOLEAN DEFAULT true NOT NULL,
    timeout_seconds DECIMAL(5, 2) DEFAULT 1.2,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_buyer_qualification_configs_buyer FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE CASCADE,
    CONSTRAINT fk_buyer_qualification_configs_vertical FOREIGN KEY (vertical_id) REFERENCES verticals(id) ON DELETE CASCADE,
    CONSTRAINT fk_buyer_qualification_configs_buyer_integration FOREIGN KEY (buyer_integration_id) REFERENCES buyer_integrations(id) ON DELETE SET NULL,
    CONSTRAINT unique_buyer_vertical_rule_set_name UNIQUE (buyer_id, vertical_id, rule_set_name)
);

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_bqc_buyer_id ON buyer_qualification_configs(buyer_id);
CREATE INDEX IF NOT EXISTS idx_bqc_vertical_id ON buyer_qualification_configs(vertical_id);
CREATE INDEX IF NOT EXISTS idx_bqc_buyer_integration_id ON buyer_qualification_configs(buyer_integration_id);
CREATE INDEX IF NOT EXISTS idx_bqc_is_active ON buyer_qualification_configs(is_active);
CREATE INDEX IF NOT EXISTS idx_bqc_rule_set_name ON buyer_qualification_configs(rule_set_name);
CREATE INDEX IF NOT EXISTS idx_bqc_buyer_id_is_active ON buyer_qualification_configs(buyer_id, is_active) WHERE is_active = true;
CREATE INDEX IF NOT EXISTS idx_bqc_buyer_vertical_enabled ON buyer_qualification_configs(buyer_id, vertical_id) WHERE enabled = true;
