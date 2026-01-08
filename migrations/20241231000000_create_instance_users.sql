-- Create instance_users table (must be created before instances, webauthn_credentials, user_otp_settings)
-- This table stores platform-level user accounts

-- Create sequence for sequential_id first (before it's referenced)
CREATE SEQUENCE IF NOT EXISTS platform_users_sequential_id_seq;

CREATE TABLE IF NOT EXISTS instance_users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sequential_id INTEGER DEFAULT nextval('platform_users_sequential_id_seq'::regclass),
    email VARCHAR NOT NULL DEFAULT '',
    encrypted_password VARCHAR NOT NULL DEFAULT '',
    confirmation_token VARCHAR,
    confirmed_at TIMESTAMP,
    confirmation_sent_at TIMESTAMP,
    reset_password_token VARCHAR,
    reset_password_sent_at TIMESTAMP,
    remember_created_at TIMESTAMP,
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    unlock_token VARCHAR,
    locked_at TIMESTAMP,
    sign_in_count INTEGER NOT NULL DEFAULT 0,
    current_sign_in_at TIMESTAMP,
    last_sign_in_at TIMESTAMP,
    current_sign_in_ip VARCHAR,
    last_sign_in_ip VARCHAR,
    first_name VARCHAR,
    last_name VARCHAR,
    phone VARCHAR,
    phone_verified_at TIMESTAMP,
    timezone VARCHAR DEFAULT 'Pacific Time (US & Canada)',
    locale VARCHAR DEFAULT 'en',
    last_password_change_at TIMESTAMP,
    status VARCHAR NOT NULL DEFAULT 'active',
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    unconfirmed_email VARCHAR,
    business_name VARCHAR,
    preferred_2fa_method VARCHAR DEFAULT 'otp',
    passwordless_login_enabled BOOLEAN DEFAULT false
);

-- Create indexes
CREATE UNIQUE INDEX IF NOT EXISTS index_instance_users_on_email ON instance_users(email);
CREATE UNIQUE INDEX IF NOT EXISTS index_instance_users_on_confirmation_token ON instance_users(confirmation_token) WHERE confirmation_token IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS index_instance_users_on_reset_password_token ON instance_users(reset_password_token) WHERE reset_password_token IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS index_instance_users_on_unlock_token ON instance_users(unlock_token) WHERE unlock_token IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS index_instance_users_on_sequential_id ON instance_users(sequential_id);
CREATE INDEX IF NOT EXISTS index_instance_users_on_status ON instance_users(status);

-- Add check constraint for status (only if it doesn't exist)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint 
        WHERE conname = 'check_platform_users_status' 
        AND conrelid = 'instance_users'::regclass
    ) THEN
        ALTER TABLE instance_users ADD CONSTRAINT check_platform_users_status 
            CHECK (status::text = ANY (ARRAY['active'::character varying, 'suspended'::character varying, 'revoked'::character varying, 'pending_verification'::character varying]::text[]));
    END IF;
END $$;
