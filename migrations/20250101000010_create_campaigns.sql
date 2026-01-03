-- Campaigns table: Publisher-buyer campaign mappings
CREATE TABLE IF NOT EXISTS campaigns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_id UUID NOT NULL,
    name VARCHAR(255),
    vertical VARCHAR(100) NOT NULL,
    campaign_token VARCHAR(255) NOT NULL UNIQUE,
    publisher_id UUID NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    is_documentation_test BOOLEAN NOT NULL DEFAULT false,
    request_type_constraint VARCHAR(50),
    revshare_percentage DECIMAL(10, 2),
    fraud_check_enabled BOOLEAN NOT NULL DEFAULT false,
    tracking_enabled BOOLEAN NOT NULL DEFAULT true,
    tie_breaker_strategy VARCHAR(50) NOT NULL DEFAULT 'random',
    deleted_at TIMESTAMPTZ,
    instance_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_campaigns_status CHECK (status IN ('active', 'paused', 'suspended', 'deleted', 'ronin'))
);

CREATE INDEX idx_campaigns_campaign_token ON campaigns(campaign_token);
CREATE INDEX idx_campaigns_buyer_id ON campaigns(buyer_id);
CREATE INDEX idx_campaigns_publisher_id ON campaigns(publisher_id);
CREATE INDEX idx_campaigns_instance_id ON campaigns(instance_id);
CREATE INDEX idx_campaigns_status ON campaigns(status);
CREATE INDEX idx_campaigns_is_documentation_test ON campaigns(is_documentation_test);
CREATE INDEX idx_campaigns_request_type_constraint ON campaigns(request_type_constraint);
CREATE INDEX idx_campaigns_deleted_at ON campaigns(deleted_at);
CREATE INDEX idx_campaigns_buyer_status ON campaigns(buyer_id, status, deleted_at) WHERE deleted_at IS NULL;
CREATE INDEX idx_campaigns_publisher_vertical_status ON campaigns(publisher_id, vertical, status);
CREATE INDEX idx_campaigns_vertical_status ON campaigns(vertical, status, deleted_at) WHERE deleted_at IS NULL;




