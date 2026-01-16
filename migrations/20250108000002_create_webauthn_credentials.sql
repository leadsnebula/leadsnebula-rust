-- Create webauthn_credentials table
-- In test mode, drop and recreate if table exists but is missing required columns (idempotent)
DO $$ 
BEGIN
    -- Check if table exists but is missing the instance_user_id column
    -- This indicates an inconsistent state from a partial migration
    IF EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'webauthn_credentials'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'
    ) THEN
        -- Drop table and all dependent objects (safe in test mode - no live data)
        DROP TABLE IF EXISTS webauthn_credentials CASCADE;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS webauthn_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_user_id UUID NOT NULL,
    external_id VARCHAR NOT NULL,
    public_key TEXT NOT NULL,
    sign_count INTEGER NOT NULL DEFAULT 0,
    name VARCHAR,
    last_used_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    passkey_type VARCHAR
);

-- Create indexes (idempotent)
CREATE UNIQUE INDEX IF NOT EXISTS index_webauthn_credentials_on_external_id ON webauthn_credentials(external_id);
CREATE INDEX IF NOT EXISTS index_webauthn_credentials_on_instance_user_id ON webauthn_credentials(instance_user_id);
CREATE INDEX IF NOT EXISTS idx_webauthn_credentials_instance_user_id ON webauthn_credentials(instance_user_id);

-- Add foreign key to instance_users table
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_webauthn_credentials_instance_user'
    ) THEN
        ALTER TABLE webauthn_credentials
            ADD CONSTRAINT fk_webauthn_credentials_instance_user
            FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_webauthn_credentials_instance_user_id'
    ) THEN
        ALTER TABLE webauthn_credentials
            ADD CONSTRAINT fk_webauthn_credentials_instance_user_id
            FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE CASCADE;
    END IF;
END $$;
