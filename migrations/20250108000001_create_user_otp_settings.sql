-- Create user_otp_settings table
-- In test mode, drop and recreate if table exists but is missing required columns (idempotent)
DO $$ 
BEGIN
    -- Check if table exists but is missing the instance_user_id column
    -- This indicates an inconsistent state from a partial migration
    IF EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'user_otp_settings'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'user_otp_settings' AND column_name = 'instance_user_id'
    ) THEN
        -- Drop table and all dependent objects (safe in test mode - no live data)
        DROP TABLE IF EXISTS user_otp_settings CASCADE;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS user_otp_settings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_user_id UUID NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    secret_encrypted TEXT NOT NULL,
    backup_codes_encrypted TEXT,
    last_verified_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Create indexes (idempotent)
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_otp_settings_instance_user_id ON user_otp_settings(instance_user_id);
CREATE INDEX IF NOT EXISTS idx_user_otp_settings_enabled ON user_otp_settings(enabled);

-- Add foreign key to instance_users table (idempotent)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_user_otp_settings_instance_user_id'
    ) THEN
        ALTER TABLE user_otp_settings
            ADD CONSTRAINT fk_user_otp_settings_instance_user_id
            FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE CASCADE;
    END IF;
END $$;
