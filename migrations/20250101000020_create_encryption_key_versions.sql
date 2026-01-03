-- Encryption key versions table: Tracks encryption key rotation
CREATE TABLE IF NOT EXISTS encryption_key_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key_type VARCHAR(100) NOT NULL,
    version INTEGER NOT NULL,
    key_hash VARCHAR(255) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    rotated_at TIMESTAMPTZ,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    rotation_job_id UUID,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT idx_encryption_key_versions_unique UNIQUE (key_type, version)
);

CREATE INDEX idx_encryption_key_versions_key_type_version ON encryption_key_versions(key_type, version);
CREATE INDEX idx_encryption_key_versions_status ON encryption_key_versions(status);
CREATE INDEX idx_encryption_key_versions_expires_at ON encryption_key_versions(expires_at);
CREATE INDEX idx_encryption_key_versions_rotation_job_id ON encryption_key_versions(rotation_job_id);




