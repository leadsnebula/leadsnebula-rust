-- Create instances table (must be created before publishers, buyers, campaigns, ping_trees)
CREATE TABLE IF NOT EXISTS instances (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    instance_user_id UUID,
    payment_status VARCHAR(20) NOT NULL DEFAULT 'trial' CHECK (payment_status IN ('trial', 'active', 'past_due', 'suspended')),
    subscription_tier VARCHAR(50),
    trial_ends_at TIMESTAMP,
    max_publishers INTEGER NOT NULL DEFAULT 999999,
    max_buyers INTEGER NOT NULL DEFAULT 999999,
    max_campaigns INTEGER NOT NULL DEFAULT 999999,
    max_leads INTEGER NOT NULL DEFAULT 999999,
    max_requests_per_hour INTEGER NOT NULL DEFAULT 999999,
    deleted_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_instances_deleted_at ON instances(deleted_at);
CREATE INDEX idx_instances_instance_user_id ON instances(instance_user_id);
CREATE INDEX idx_instances_payment_status ON instances(payment_status);

