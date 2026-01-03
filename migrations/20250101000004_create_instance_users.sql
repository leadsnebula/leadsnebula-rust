-- Instance users table: User accounts for the platform
-- Create sequence first
CREATE SEQUENCE IF NOT EXISTS instance_users_sequential_id_seq;

CREATE TABLE IF NOT EXISTS instance_users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sequential_id INTEGER UNIQUE DEFAULT nextval('instance_users_sequential_id_seq'),
    email VARCHAR(255) NOT NULL UNIQUE,
    encrypted_password VARCHAR(255) NOT NULL DEFAULT '',
    confirmation_token VARCHAR(255) UNIQUE,
    confirmed_at TIMESTAMPTZ,
    confirmation_sent_at TIMESTAMPTZ,
    reset_password_token VARCHAR(255) UNIQUE,
    reset_password_sent_at TIMESTAMPTZ,
    remember_created_at TIMESTAMPTZ,
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    unlock_token VARCHAR(255) UNIQUE,
    locked_at TIMESTAMPTZ,
    sign_in_count INTEGER NOT NULL DEFAULT 0,
    current_sign_in_at TIMESTAMPTZ,
    last_sign_in_at TIMESTAMPTZ,
    current_sign_in_ip VARCHAR(45),
    last_sign_in_ip VARCHAR(45),
    first_name VARCHAR(255),
    last_name VARCHAR(255),
    phone VARCHAR(50),
    phone_verified_at TIMESTAMPTZ,
    timezone VARCHAR(100) NOT NULL DEFAULT 'Pacific Time (US & Canada)',
    locale VARCHAR(10) NOT NULL DEFAULT 'en',
    last_password_change_at TIMESTAMPTZ,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    unconfirmed_email VARCHAR(255),
    business_name VARCHAR(255),
    preferred_2fa_method VARCHAR(50) NOT NULL DEFAULT 'otp',
    passwordless_login_enabled BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_instance_users_status CHECK (status IN ('active', 'suspended', 'revoked', 'pending_verification'))
);

CREATE INDEX idx_instance_users_email ON instance_users(email);
CREATE INDEX idx_instance_users_sequential_id ON instance_users(sequential_id);
CREATE INDEX idx_instance_users_status ON instance_users(status);
CREATE INDEX idx_instance_users_confirmation_token ON instance_users(confirmation_token);
CREATE INDEX idx_instance_users_reset_password_token ON instance_users(reset_password_token);
CREATE INDEX idx_instance_users_unlock_token ON instance_users(unlock_token);

