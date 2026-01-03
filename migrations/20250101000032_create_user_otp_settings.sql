-- User OTP Settings table: TOTP/2FA configuration for users
CREATE TABLE IF NOT EXISTS user_otp_settings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_user_id UUID NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    secret_encrypted TEXT NOT NULL,
    backup_codes_encrypted TEXT,
    last_verified_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_user_otp_settings_instance_user_id
        FOREIGN KEY (instance_user_id)
        REFERENCES instance_users(id)
        ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_user_otp_settings_instance_user_id ON user_otp_settings(instance_user_id);
CREATE INDEX idx_user_otp_settings_enabled ON user_otp_settings(enabled);


