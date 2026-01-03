-- Encryption decryption failures table: Tracks decryption errors
CREATE TABLE IF NOT EXISTS encryption_decryption_failures (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    model_name VARCHAR(255) NOT NULL,
    record_id UUID NOT NULL,
    encrypted_attribute VARCHAR(255) NOT NULL,
    key_version INTEGER,
    error_type VARCHAR(100) NOT NULL,
    error_message TEXT NOT NULL,
    error_backtrace TEXT,
    status VARCHAR(50) NOT NULL DEFAULT 'unresolved',
    resolved_at TIMESTAMPTZ,
    resolution_notes TEXT,
    failure_count INTEGER NOT NULL DEFAULT 1,
    first_failed_at TIMESTAMPTZ NOT NULL,
    last_failed_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT idx_encryption_decryption_failures_unique UNIQUE (model_name, record_id, encrypted_attribute)
);

CREATE INDEX idx_encryption_decryption_failures_status ON encryption_decryption_failures(status);
CREATE INDEX idx_encryption_decryption_failures_key_version ON encryption_decryption_failures(key_version);
CREATE INDEX idx_encryption_decryption_failures_last_failed_at ON encryption_decryption_failures(last_failed_at);




