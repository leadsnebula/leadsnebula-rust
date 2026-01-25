-- Create webauthn_credentials table
-- Drop and recreate if table exists but is missing required columns (idempotent)
-- This handles partial migrations where table exists but is missing platform_user_id or instance_user_id
DO $$ 
DECLARE
    table_exists BOOLEAN;
    has_platform_user_id BOOLEAN;
    has_instance_user_id BOOLEAN;
BEGIN
    -- Check if table exists
    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'webauthn_credentials'
    ) INTO table_exists;
    
    IF table_exists THEN
        -- Check if required columns exist
        SELECT EXISTS (
            SELECT 1 FROM information_schema.columns 
            WHERE table_name = 'webauthn_credentials' AND column_name = 'platform_user_id'
        ) INTO has_platform_user_id;
        
        SELECT EXISTS (
            SELECT 1 FROM information_schema.columns 
            WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'
        ) INTO has_instance_user_id;
        
        -- If table exists but is missing platform_user_id (required) or instance_user_id, drop and recreate
        -- platform_user_id is REQUIRED (NOT NULL), so missing it means inconsistent state
        IF NOT has_platform_user_id OR NOT has_instance_user_id THEN
            -- Drop table and all dependent objects (safe in test/CI - ephemeral databases)
            -- In production, this should be handled by fix_partial_migrations migration
            DROP TABLE IF EXISTS webauthn_credentials CASCADE;
        END IF;
    END IF;
END $$;

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

-- Create indexes (idempotent)
-- Only create indexes if columns exist (defensive check)
DO $$
BEGIN
    -- external_id index (always exists)
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'external_id'
    ) THEN
        CREATE UNIQUE INDEX IF NOT EXISTS index_webauthn_credentials_on_external_id ON webauthn_credentials(external_id);
    END IF;
    
    -- platform_user_id index (required column)
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'platform_user_id'
    ) THEN
        CREATE INDEX IF NOT EXISTS index_webauthn_credentials_on_platform_user_id ON webauthn_credentials(platform_user_id);
    END IF;
    
    -- instance_user_id index (optional column)
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'
    ) THEN
        CREATE INDEX IF NOT EXISTS index_webauthn_credentials_on_instance_user_id ON webauthn_credentials(instance_user_id);
        CREATE INDEX IF NOT EXISTS idx_webauthn_credentials_instance_user_id ON webauthn_credentials(instance_user_id);
    END IF;
END $$;

-- Add foreign key constraints to instance_users table
-- Only add constraints if columns exist (defensive check)
DO $$
BEGIN
    -- Foreign key for platform_user_id (primary, NOT NULL, REQUIRED)
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'platform_user_id'
    ) AND NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_webauthn_credentials_platform_user'
    ) THEN
        ALTER TABLE webauthn_credentials
            ADD CONSTRAINT fk_webauthn_credentials_platform_user
            FOREIGN KEY (platform_user_id) REFERENCES instance_users(id) ON DELETE CASCADE;
    END IF;
    
    -- Foreign key for instance_user_id (optional, nullable)
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'
    ) THEN
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
    END IF;
END $$;
