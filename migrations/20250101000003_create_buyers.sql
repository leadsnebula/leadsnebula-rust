-- Create buyers table
CREATE TABLE IF NOT EXISTS buyers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    instance_id UUID NOT NULL,
    instance_user_id UUID,
    vertical_id UUID,
    buyer_integration_id UUID,
    status VARCHAR(20) NOT NULL DEFAULT 'incomplete',
    deleted_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_buyers_instance_id ON buyers(instance_id);
CREATE INDEX idx_buyers_vertical_id ON buyers(vertical_id);
CREATE INDEX idx_buyers_status ON buyers(status);
CREATE INDEX idx_buyers_deleted_at ON buyers(deleted_at);

