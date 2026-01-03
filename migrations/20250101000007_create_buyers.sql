-- Buyers table: Lead buyers (destinations)
CREATE TABLE IF NOT EXISTS buyers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    instance_user_id UUID,
    instance_id UUID NOT NULL,
    contact_info JSONB NOT NULL DEFAULT '{}',
    deleted_at TIMESTAMPTZ,
    ein_tin VARCHAR(50),
    address_street TEXT,
    address_city VARCHAR(255),
    address_state VARCHAR(50),
    address_zip VARCHAR(20),
    email_address VARCHAR(255),
    representative_first_name VARCHAR(255),
    representative_last_name VARCHAR(255),
    documents JSONB NOT NULL DEFAULT '[]',
    post_type VARCHAR(50) NOT NULL DEFAULT 'full_post',
    buyer_type VARCHAR(50),
    status VARCHAR(50) NOT NULL DEFAULT 'incomplete',
    vertical_id UUID,
    buyer_integration_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT idx_buyers_instance_name UNIQUE (instance_id, name)
);

CREATE INDEX idx_buyers_instance_id ON buyers(instance_id);
CREATE INDEX idx_buyers_instance_user_id ON buyers(instance_user_id);
CREATE INDEX idx_buyers_vertical_id ON buyers(vertical_id);
CREATE INDEX idx_buyers_buyer_integration_id ON buyers(buyer_integration_id);
CREATE INDEX idx_buyers_status ON buyers(status);
CREATE INDEX idx_buyers_post_type ON buyers(post_type);
CREATE INDEX idx_buyers_buyer_type ON buyers(buyer_type);
CREATE INDEX idx_buyers_deleted_at ON buyers(deleted_at);
CREATE INDEX idx_buyers_ein_tin ON buyers(ein_tin);
CREATE INDEX idx_buyers_email_address ON buyers(email_address);




