-- Create ping_tree_publishers join table
-- Enables multiple publishers to share a single ping tree
-- Stores per-publisher revenue share configuration

CREATE TABLE IF NOT EXISTS ping_tree_publishers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ping_tree_id UUID NOT NULL REFERENCES ping_trees(id) ON DELETE CASCADE,
    publisher_id UUID NOT NULL REFERENCES publishers(id) ON DELETE CASCADE,
    vertical VARCHAR(50) NOT NULL,
    revshare_percentage DECIMAL(5,2),
    revshare_flat_amount DECIMAL(10,2),
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    
    -- One ping tree per publisher per vertical
    UNIQUE(publisher_id, vertical),
    
    -- Prevent duplicate assignments
    UNIQUE(ping_tree_id, publisher_id),
    
    -- CHECK constraints for revshare validation (defense in depth)
    CONSTRAINT chk_revshare_percentage_range 
      CHECK (revshare_percentage IS NULL OR (revshare_percentage >= 0.0 AND revshare_percentage <= 100.0)),
    CONSTRAINT chk_revshare_flat_non_negative 
      CHECK (revshare_flat_amount IS NULL OR revshare_flat_amount >= 0.0),
    CONSTRAINT chk_revshare_exclusive 
      CHECK (
        (revshare_percentage IS NOT NULL AND revshare_flat_amount IS NULL) OR
        (revshare_percentage IS NULL AND revshare_flat_amount IS NOT NULL) OR
        (revshare_percentage IS NULL AND revshare_flat_amount IS NULL)
      )
);

-- Index for routing queries (critical path)
CREATE INDEX IF NOT EXISTS idx_ptp_routing ON ping_tree_publishers(publisher_id, vertical);

-- Index for ping tree lookups
CREATE INDEX IF NOT EXISTS idx_ptp_ping_tree_id ON ping_tree_publishers(ping_tree_id);
