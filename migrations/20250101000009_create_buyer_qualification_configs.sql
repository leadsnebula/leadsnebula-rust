-- Buyer qualification configs table: Rules for buyer qualification
CREATE TABLE IF NOT EXISTS buyer_qualification_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_id UUID NOT NULL,
    vertical_id UUID NOT NULL,
    config JSONB NOT NULL DEFAULT '{}',
    rules_order TEXT[] NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT true,
    rule_set_name VARCHAR(255),
    is_active BOOLEAN NOT NULL DEFAULT true,
    buyer_integration_id UUID,
    timeout_seconds DECIMAL(5, 2) NOT NULL DEFAULT 1.2,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT idx_buyer_qualification_configs_unique UNIQUE (buyer_id, vertical_id, rule_set_name)
);

CREATE INDEX idx_buyer_qualification_configs_buyer_id ON buyer_qualification_configs(buyer_id);
CREATE INDEX idx_buyer_qualification_configs_vertical_id ON buyer_qualification_configs(vertical_id);
CREATE INDEX idx_buyer_qualification_configs_buyer_integration_id ON buyer_qualification_configs(buyer_integration_id);
CREATE INDEX idx_buyer_qualification_configs_is_active ON buyer_qualification_configs(is_active);
CREATE INDEX idx_buyer_qualification_configs_rule_set_name ON buyer_qualification_configs(rule_set_name);
CREATE INDEX idx_buyer_qualification_configs_buyer_vertical_enabled ON buyer_qualification_configs(buyer_id, vertical_id) WHERE enabled = true;
CREATE INDEX idx_buyer_qualification_configs_buyer_active ON buyer_qualification_configs(buyer_id, is_active) WHERE is_active = true;




