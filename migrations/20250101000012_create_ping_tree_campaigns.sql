-- Ping tree campaigns table: Campaigns in ping trees
CREATE TABLE IF NOT EXISTS ping_tree_campaigns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ping_tree_id UUID NOT NULL,
    campaign_id UUID NOT NULL,
    priority INTEGER,
    min_price DECIMAL(10, 2),
    max_price DECIMAL(10, 2),
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT idx_ping_tree_campaigns_unique UNIQUE (ping_tree_id, campaign_id)
);

CREATE INDEX idx_ping_tree_campaigns_ping_tree_id ON ping_tree_campaigns(ping_tree_id);
CREATE INDEX idx_ping_tree_campaigns_campaign_id ON ping_tree_campaigns(campaign_id);
CREATE INDEX idx_ping_tree_campaigns_ordering ON ping_tree_campaigns(ping_tree_id, priority);
CREATE INDEX idx_ping_tree_campaigns_routing ON ping_tree_campaigns(ping_tree_id, enabled, priority) WHERE enabled = true;




