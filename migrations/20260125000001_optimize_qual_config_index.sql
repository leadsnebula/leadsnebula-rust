-- Optimize qualification config query performance
-- Add composite index for buyer_id + enabled + is_active lookup (used in find_by_buyer_ids)

-- Composite index for the exact query pattern: WHERE buyer_id = ANY($1) AND enabled = true AND is_active = true
CREATE INDEX IF NOT EXISTS idx_bqc_buyer_enabled_active 
ON buyer_qualification_configs(buyer_id, enabled, is_active) 
WHERE enabled = true AND is_active = true;

-- This index optimizes the EXISTS check and full query in BuyerQualificationConfig::find_by_buyer_ids
-- The partial index (WHERE enabled = true AND is_active = true) reduces index size and improves query speed
