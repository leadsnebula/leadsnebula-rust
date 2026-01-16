-- Create ping_tree_campaigns table (join table)
CREATE TABLE IF NOT EXISTS ping_tree_campaigns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ping_tree_id UUID NOT NULL,
    campaign_id UUID NOT NULL,
    priority INTEGER,
    min_price DECIMAL(10, 2),
    max_price DECIMAL(10, 2),
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_ping_tree_campaigns_ping_tree FOREIGN KEY (ping_tree_id) REFERENCES ping_trees(id) ON DELETE CASCADE,
    CONSTRAINT fk_ping_tree_campaigns_campaign FOREIGN KEY (campaign_id) REFERENCES campaigns(id) ON DELETE CASCADE,
    CONSTRAINT unique_ping_tree_campaign UNIQUE (ping_tree_id, campaign_id)
);

CREATE INDEX IF NOT EXISTS idx_ping_tree_campaigns_ping_tree_id ON ping_tree_campaigns(ping_tree_id);
CREATE INDEX IF NOT EXISTS idx_ping_tree_campaigns_campaign_id ON ping_tree_campaigns(campaign_id);
CREATE INDEX IF NOT EXISTS idx_ping_tree_campaigns_routing ON ping_tree_campaigns(ping_tree_id, enabled, priority) WHERE enabled = true;
CREATE INDEX IF NOT EXISTS idx_ping_tree_campaigns_ordering ON ping_tree_campaigns(ping_tree_id, priority);

