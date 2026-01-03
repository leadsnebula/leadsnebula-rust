-- WebAuthn Credentials table: Passkeys for passwordless authentication
CREATE TABLE IF NOT EXISTS webauthn_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_user_id UUID NOT NULL,
    external_id VARCHAR(255) NOT NULL UNIQUE,
    public_key TEXT NOT NULL,
    sign_count INTEGER NOT NULL DEFAULT 0,
    name VARCHAR(255),
    last_used_at TIMESTAMPTZ,
    passkey_type VARCHAR(50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_webauthn_credentials_instance_user_id
        FOREIGN KEY (instance_user_id)
        REFERENCES instance_users(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_webauthn_credentials_external_id ON webauthn_credentials(external_id);
CREATE INDEX idx_webauthn_credentials_instance_user_id ON webauthn_credentials(instance_user_id);
CREATE INDEX idx_webauthn_credentials_passkey_type ON webauthn_credentials(passkey_type);


