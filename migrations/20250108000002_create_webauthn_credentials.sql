-- Create webauthn_credentials table
CREATE TABLE IF NOT EXISTS webauthn_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    platform_user_id UUID NOT NULL,
    instance_user_id UUID,
    external_id VARCHAR NOT NULL,
    public_key TEXT NOT NULL,
    sign_count INTEGER NOT NULL DEFAULT 0,
    name VARCHAR,
    last_used_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    passkey_type VARCHAR
);

CREATE UNIQUE INDEX index_webauthn_credentials_on_external_id ON webauthn_credentials(external_id);
CREATE INDEX index_webauthn_credentials_on_platform_user_id ON webauthn_credentials(platform_user_id);
CREATE INDEX index_webauthn_credentials_on_instance_user_id ON webauthn_credentials(instance_user_id);

-- Add foreign keys to instance_users table
ALTER TABLE webauthn_credentials
    ADD CONSTRAINT fk_webauthn_credentials_platform_user
    FOREIGN KEY (platform_user_id) REFERENCES instance_users(id);
    
ALTER TABLE webauthn_credentials
    ADD CONSTRAINT fk_webauthn_credentials_instance_user
    FOREIGN KEY (instance_user_id) REFERENCES instance_users(id);
