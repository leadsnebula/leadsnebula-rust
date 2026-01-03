-- Password histories table: Tracks password history for policy enforcement
CREATE TABLE IF NOT EXISTS password_histories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_user_id UUID NOT NULL,
    encrypted_password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_password_histories_instance_user_id ON password_histories(instance_user_id);
CREATE INDEX idx_password_histories_user_created_at ON password_histories(instance_user_id, created_at DESC);




