-- Verticals table: Defines lead verticals (solar, insurance, etc.)
CREATE TABLE IF NOT EXISTS verticals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL,
    slug VARCHAR(50) NOT NULL UNIQUE,
    description TEXT,
    is_active BOOLEAN NOT NULL DEFAULT true,
    required_fields JSONB NOT NULL DEFAULT '{}',
    optional_fields JSONB NOT NULL DEFAULT '{}',
    settings JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_verticals_slug ON verticals(slug);
CREATE INDEX idx_verticals_is_active ON verticals(is_active);
CREATE INDEX idx_verticals_name ON verticals(name);




