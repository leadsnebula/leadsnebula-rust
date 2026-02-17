-- Allow multiple publishers per campaign (add/remove without changing primary).
-- campaigns.publisher_id remains the "primary" publisher; campaign_publishers holds additional publishers.
-- Lead routing considers a campaign valid for a publisher if c.publisher_id = publisher OR cp.publisher_id = publisher.

CREATE TABLE IF NOT EXISTS campaign_publishers (
    campaign_id UUID NOT NULL REFERENCES campaigns(id) ON DELETE CASCADE,
    publisher_id UUID NOT NULL REFERENCES publishers(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (campaign_id, publisher_id)
);

CREATE INDEX IF NOT EXISTS idx_campaign_publishers_campaign_id ON campaign_publishers(campaign_id);
CREATE INDEX IF NOT EXISTS idx_campaign_publishers_publisher_id ON campaign_publishers(publisher_id);

COMMENT ON TABLE campaign_publishers IS 'Additional publishers for a campaign; campaigns.publisher_id is the primary publisher.';
