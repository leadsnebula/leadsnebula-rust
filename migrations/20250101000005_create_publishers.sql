-- Publishers table: Lead publishers (sources)
CREATE TABLE IF NOT EXISTS publishers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    api_key_hash VARCHAR(255) NOT NULL UNIQUE,
    api_key_prefix VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    total_requests INTEGER NOT NULL DEFAULT 0,
    last_request_at TIMESTAMPTZ,
    instance_user_id UUID,
    is_documentation_test BOOLEAN NOT NULL DEFAULT false,
    instance_id UUID NOT NULL,
    hmac_secret_hash VARCHAR(255),
    hmac_secret_prefix VARCHAR(50),
    hmac_required BOOLEAN NOT NULL DEFAULT false,
    deleted_at TIMESTAMPTZ,
    hmac_secret_encrypted TEXT,
    api_key_encrypted TEXT,
    ein_tin VARCHAR(50),
    address_street TEXT,
    address_city VARCHAR(255),
    address_state VARCHAR(50),
    address_zip VARCHAR(20),
    representative_first_name VARCHAR(255),
    representative_last_name VARCHAR(255),
    timezone VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_publishers_email ON publishers(email);
CREATE INDEX idx_publishers_api_key_hash ON publishers(api_key_hash);
CREATE INDEX idx_publishers_instance_id ON publishers(instance_id);
CREATE INDEX idx_publishers_instance_user_id ON publishers(instance_user_id);
CREATE INDEX idx_publishers_status ON publishers(status);
CREATE INDEX idx_publishers_is_documentation_test ON publishers(is_documentation_test);
CREATE INDEX idx_publishers_deleted_at ON publishers(deleted_at);
CREATE INDEX idx_publishers_ein_tin ON publishers(ein_tin);




