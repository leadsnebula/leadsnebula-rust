-- Fix partially applied migrations in main/dev databases
-- This migration detects and fixes inconsistent database state from partial migrations
-- Safe to run multiple times (idempotent)

-- ============================================
-- STEP 1: Fix user_otp_settings table
-- ============================================
-- If table exists but is missing instance_user_id column, fix it
DO $$ 
DECLARE
    table_exists BOOLEAN;
    column_exists BOOLEAN;
    row_count BIGINT;
BEGIN
    -- Check if table exists
    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'user_otp_settings'
    ) INTO table_exists;
    
    -- Check if column exists
    SELECT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'user_otp_settings' AND column_name = 'instance_user_id'
    ) INTO column_exists;
    
    -- If table exists but column is missing
    IF table_exists AND NOT column_exists THEN
        -- Check if table has data
        EXECUTE 'SELECT COUNT(*) FROM user_otp_settings' INTO row_count;
        
        IF row_count > 0 THEN
            RAISE WARNING 'user_otp_settings table has % rows but missing instance_user_id column - data will be lost when fixing', row_count;
        END IF;
        
        RAISE NOTICE 'Fixing user_otp_settings: table exists but missing instance_user_id column - dropping and will recreate';
        DROP TABLE IF EXISTS user_otp_settings CASCADE;
    END IF;
END $$;

-- Recreate table if it doesn't exist (migration 20250108000001 will handle this if needed)
-- But we ensure it has the correct structure here
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

-- Ensure instance_user_id column exists (add if missing)
DO $$ 
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'user_otp_settings' AND column_name = 'instance_user_id'
    ) THEN
        ALTER TABLE user_otp_settings ADD COLUMN instance_user_id UUID NOT NULL;
    END IF;
END $$;

-- Ensure other required columns exist
DO $$ 
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'user_otp_settings' AND column_name = 'enabled'
    ) THEN
        ALTER TABLE user_otp_settings ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT false;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'user_otp_settings' AND column_name = 'secret_encrypted'
    ) THEN
        ALTER TABLE user_otp_settings ADD COLUMN secret_encrypted TEXT NOT NULL DEFAULT '';
        ALTER TABLE user_otp_settings ALTER COLUMN secret_encrypted DROP DEFAULT;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'user_otp_settings' AND column_name = 'backup_codes_encrypted'
    ) THEN
        ALTER TABLE user_otp_settings ADD COLUMN backup_codes_encrypted TEXT;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'user_otp_settings' AND column_name = 'last_verified_at'
    ) THEN
        ALTER TABLE user_otp_settings ADD COLUMN last_verified_at TIMESTAMP;
    END IF;
END $$;

-- Create indexes if they don't exist
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_otp_settings_instance_user_id ON user_otp_settings(instance_user_id);
CREATE INDEX IF NOT EXISTS idx_user_otp_settings_enabled ON user_otp_settings(enabled);

-- Add foreign key constraint if it doesn't exist
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

-- ============================================
-- STEP 2: Fix webauthn_credentials table
-- ============================================
-- If table exists but is missing instance_user_id column, fix it
DO $$ 
DECLARE
    table_exists BOOLEAN;
    column_exists BOOLEAN;
    row_count BIGINT;
BEGIN
    -- Check if table exists
    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'webauthn_credentials'
    ) INTO table_exists;
    
    -- Check if column exists
    SELECT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'
    ) INTO column_exists;
    
    -- If table exists but column is missing
    IF table_exists AND NOT column_exists THEN
        -- Check if table has data
        EXECUTE 'SELECT COUNT(*) FROM webauthn_credentials' INTO row_count;
        
        IF row_count > 0 THEN
            RAISE WARNING 'webauthn_credentials table has % rows but missing instance_user_id column - data will be lost when fixing', row_count;
        END IF;
        
        RAISE NOTICE 'Fixing webauthn_credentials: table exists but missing instance_user_id column - dropping and will recreate';
        DROP TABLE IF EXISTS webauthn_credentials CASCADE;
    END IF;
END $$;

-- Recreate table if it doesn't exist
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

-- Ensure instance_user_id column exists (add if missing)
DO $$ 
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'
    ) THEN
        ALTER TABLE webauthn_credentials ADD COLUMN instance_user_id UUID NOT NULL;
    END IF;
END $$;

-- Ensure other required columns exist
DO $$ 
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'external_id'
    ) THEN
        ALTER TABLE webauthn_credentials ADD COLUMN external_id VARCHAR NOT NULL DEFAULT '';
        ALTER TABLE webauthn_credentials ALTER COLUMN external_id DROP DEFAULT;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'public_key'
    ) THEN
        ALTER TABLE webauthn_credentials ADD COLUMN public_key TEXT NOT NULL DEFAULT '';
        ALTER TABLE webauthn_credentials ALTER COLUMN public_key DROP DEFAULT;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'sign_count'
    ) THEN
        ALTER TABLE webauthn_credentials ADD COLUMN sign_count INTEGER NOT NULL DEFAULT 0;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'name'
    ) THEN
        ALTER TABLE webauthn_credentials ADD COLUMN name VARCHAR;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'last_used_at'
    ) THEN
        ALTER TABLE webauthn_credentials ADD COLUMN last_used_at TIMESTAMP;
    END IF;
    
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'webauthn_credentials' AND column_name = 'passkey_type'
    ) THEN
        ALTER TABLE webauthn_credentials ADD COLUMN passkey_type VARCHAR;
    END IF;
END $$;

-- Create indexes if they don't exist
CREATE UNIQUE INDEX IF NOT EXISTS index_webauthn_credentials_on_external_id ON webauthn_credentials(external_id);
CREATE INDEX IF NOT EXISTS index_webauthn_credentials_on_instance_user_id ON webauthn_credentials(instance_user_id);
CREATE INDEX IF NOT EXISTS idx_webauthn_credentials_instance_user_id ON webauthn_credentials(instance_user_id);

-- Add foreign key constraints if they don't exist
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

-- ============================================
-- STEP 3: Clean up invalid foreign key constraints
-- ============================================
-- Remove foreign key constraints that reference non-existent columns
DO $$ 
DECLARE
    r RECORD;
BEGIN
    FOR r IN 
        SELECT 
            conname,
            conrelid::regclass::text as table_name,
            pg_get_constraintdef(oid) as constraint_def
        FROM pg_constraint
        WHERE contype = 'f'
        AND conrelid IN (
            SELECT oid FROM pg_class WHERE relname IN ('user_otp_settings', 'webauthn_credentials')
        )
    LOOP
        -- Check if the constraint references a column that doesn't exist
        IF NOT EXISTS (
            SELECT 1 FROM pg_attribute a
            JOIN pg_constraint c ON a.attrelid = c.conrelid
            WHERE c.conname = r.conname
            AND a.attnum = ANY(c.conkey)
        ) THEN
            RAISE NOTICE 'Removing invalid foreign key constraint: % on table %', r.conname, r.table_name;
            EXECUTE format('ALTER TABLE %I DROP CONSTRAINT IF EXISTS %I CASCADE', r.table_name, r.conname);
        END IF;
    END LOOP;
END $$;

-- ============================================
-- STEP 4: Clean up orphaned migration records
-- ============================================
-- Remove migration records for files that no longer exist
DO $$ 
DECLARE
    orphaned_versions BIGINT[];
    v BIGINT;
BEGIN
    -- Find orphaned migration records (applied but file no longer exists)
    -- Common case: migration 20250101000010 was deleted
    SELECT ARRAY_AGG(version)
    INTO orphaned_versions
    FROM _sqlx_migrations
    WHERE version NOT IN (
        -- List of all existing migration versions
        20241231000000,
        20250101000000, 20250101000001, 20250101000002, 20250101000003, 20250101000004,
        20250101000005, 20250101000006, 20250101000007, 20250101000008, 20250101000009,
        20250105000001, 20250105000002, 20250105000003,
        20250106000001, 20250106000002,
        20250107000001, 20250107000002, 20250107000003, 20250107000004,
        20250108000001, 20250108000002,
        20250115000001, 20250115000002, 20250115000003, 20250115000004, 20250115000005,
        20250116000001, 20250116000002,
        20260112000001, 20260112000002, 20260112000003, 20260112000004,
        20260113000001, 20260113000002, 20260113000003, 20260113000004,
        20260114000001,
        20260115000000
    );
    
    IF orphaned_versions IS NOT NULL AND array_length(orphaned_versions, 1) > 0 THEN
        RAISE NOTICE 'Removing % orphaned migration record(s): %', array_length(orphaned_versions, 1), orphaned_versions;
        FOREACH v IN ARRAY orphaned_versions
        LOOP
            DELETE FROM _sqlx_migrations WHERE version = v;
            RAISE NOTICE 'Removed orphaned migration record: %', v;
        END LOOP;
    ELSE
        RAISE NOTICE 'No orphaned migration records found';
    END IF;
END $$;

-- ============================================
-- STEP 5: Verify and report final state
-- ============================================
DO $$ 
DECLARE
    user_otp_ok BOOLEAN;
    webauthn_ok BOOLEAN;
BEGIN
    -- Check user_otp_settings
    SELECT 
        EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'user_otp_settings')
        AND EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'user_otp_settings' AND column_name = 'instance_user_id')
    INTO user_otp_ok;
    
    -- Check webauthn_credentials
    SELECT 
        EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'webauthn_credentials')
        AND EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id')
    INTO webauthn_ok;
    
    IF user_otp_ok AND webauthn_ok THEN
        RAISE NOTICE '✅ All partial migrations fixed successfully';
    ELSE
        IF NOT user_otp_ok THEN
            RAISE WARNING '⚠️  user_otp_settings table still has issues';
        END IF;
        IF NOT webauthn_ok THEN
            RAISE WARNING '⚠️  webauthn_credentials table still has issues';
        END IF;
    END IF;
END $$;
