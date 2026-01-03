-- Lead sales table: Tracks lead sales through ping/post cycle
CREATE TABLE IF NOT EXISTS lead_sales (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lead_id UUID NOT NULL,
    buyer_id UUID NOT NULL,
    campaign_id UUID NOT NULL,
    ping_id VARCHAR(255),
    ping_bid_amount DECIMAL(10, 2),
    ping_response_time_ms INTEGER,
    ping_rank INTEGER,
    post_id VARCHAR(255),
    post_status VARCHAR(20),
    post_sale_price DECIMAL(10, 2),
    post_sold_at TIMESTAMPTZ,
    status VARCHAR(20) NOT NULL,
    sequence_order INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_lead_sales_status CHECK (status IN ('pinged', 'bid_accepted', 'posted', 'sold', 'rejected', 'skipped'))
);

CREATE INDEX idx_lead_sales_lead_id ON lead_sales(lead_id);
CREATE INDEX idx_lead_sales_buyer_id ON lead_sales(buyer_id);
CREATE INDEX idx_lead_sales_campaign_id ON lead_sales(campaign_id);
CREATE INDEX idx_lead_sales_status ON lead_sales(status);
CREATE INDEX idx_lead_sales_lead_ping_rank ON lead_sales(lead_id, ping_rank);
CREATE INDEX idx_lead_sales_lead_sequence_order ON lead_sales(lead_id, sequence_order);




