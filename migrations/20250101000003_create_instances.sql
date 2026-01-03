-- Instances table: Multi-tenant instance management
CREATE TABLE IF NOT EXISTS instances (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    instance_user_id UUID NOT NULL,
    payment_status VARCHAR(50) NOT NULL DEFAULT 'trial',
    subscription_tier VARCHAR(100),
    trial_ends_at TIMESTAMPTZ,
    max_publishers INTEGER NOT NULL DEFAULT 999999,
    max_buyers INTEGER NOT NULL DEFAULT 999999,
    max_campaigns INTEGER NOT NULL DEFAULT 999999,
    max_leads INTEGER NOT NULL DEFAULT 999999,
    max_requests_per_hour INTEGER NOT NULL DEFAULT 999999,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_instances_payment_status CHECK (payment_status IN ('trial', 'active', 'past_due', 'suspended'))
);

CREATE INDEX idx_instances_instance_user_id ON instances(instance_user_id);
CREATE INDEX idx_instances_payment_status ON instances(payment_status);
CREATE INDEX idx_instances_deleted_at ON instances(deleted_at);




