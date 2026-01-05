-- Create publishers table
CREATE TABLE IF NOT EXISTS publishers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    api_key_hash VARCHAR(64) NOT NULL UNIQUE,
    api_key_prefix VARCHAR(20) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    total_requests INTEGER NOT NULL DEFAULT 0,
    last_request_at TIMESTAMP,
    instance_id UUID NOT NULL,
    instance_user_id UUID,
    is_documentation_test BOOLEAN NOT NULL DEFAULT false,
    hmac_secret_hash VARCHAR(64),
    hmac_secret_prefix VARCHAR(20),
    hmac_required BOOLEAN NOT NULL DEFAULT false,
    hmac_secret_encrypted TEXT,
    deleted_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_publishers_api_key_hash ON publishers(api_key_hash);
CREATE INDEX idx_publishers_status ON publishers(status);
CREATE INDEX idx_publishers_deleted_at ON publishers(deleted_at);
CREATE INDEX idx_publishers_instance_id ON publishers(instance_id);

