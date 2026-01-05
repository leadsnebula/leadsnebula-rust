-- Create campaigns table
CREATE TABLE IF NOT EXISTS campaigns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_id UUID NOT NULL,
    publisher_id UUID NOT NULL,
    instance_id UUID NOT NULL,
    name VARCHAR(255),
    vertical VARCHAR(50) NOT NULL,
    campaign_token VARCHAR(255) NOT NULL UNIQUE,
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    is_documentation_test BOOLEAN NOT NULL DEFAULT false,
    deleted_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_campaigns_campaign_token ON campaigns(campaign_token);
CREATE INDEX idx_campaigns_buyer_id ON campaigns(buyer_id);
CREATE INDEX idx_campaigns_publisher_id ON campaigns(publisher_id);
CREATE INDEX idx_campaigns_status ON campaigns(status);
CREATE INDEX idx_campaigns_deleted_at ON campaigns(deleted_at);

