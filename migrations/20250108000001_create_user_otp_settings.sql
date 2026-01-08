-- Create user_otp_settings table
CREATE TABLE IF NOT EXISTS user_otp_settings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    platform_user_id UUID NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    secret TEXT NOT NULL,
    backup_codes TEXT,
    last_verified_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_user_otp_settings_platform_user_id ON user_otp_settings(platform_user_id);

-- Add foreign key to instance_users table
ALTER TABLE user_otp_settings
    ADD CONSTRAINT fk_user_otp_settings_platform_user
    FOREIGN KEY (platform_user_id) REFERENCES instance_users(id);
