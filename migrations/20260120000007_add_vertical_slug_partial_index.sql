-- Add partial index on verticals(slug) WHERE is_active = true for optimal performance
-- This index significantly speeds up vertical lookups in carina.rs
-- Note: verticals table does not have deleted_at (no soft deletes for verticals)
CREATE INDEX IF NOT EXISTS idx_verticals_slug_active 
ON verticals(slug) 
WHERE is_active = true;
