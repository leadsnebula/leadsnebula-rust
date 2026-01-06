-- Create publisher_verticals many-to-many relationship table
CREATE TABLE IF NOT EXISTS publisher_verticals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    publisher_id UUID NOT NULL REFERENCES publishers(id) ON DELETE CASCADE,
    vertical_id UUID NOT NULL REFERENCES verticals(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE(publisher_id, vertical_id)
);

CREATE INDEX idx_publisher_verticals_publisher_id ON publisher_verticals(publisher_id);
CREATE INDEX idx_publisher_verticals_vertical_id ON publisher_verticals(vertical_id);
