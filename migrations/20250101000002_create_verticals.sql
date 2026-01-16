-- Create verticals table
CREATE TABLE IF NOT EXISTS verticals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(50) NOT NULL UNIQUE,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_verticals_slug ON verticals(slug);
CREATE INDEX IF NOT EXISTS idx_verticals_is_active ON verticals(is_active);

-- Insert default solar vertical
INSERT INTO verticals (name, slug, is_active) VALUES ('Solar', 'solar', true) ON CONFLICT (slug) DO NOTHING;

