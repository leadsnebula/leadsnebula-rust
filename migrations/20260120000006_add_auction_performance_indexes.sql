-- Performance indexes for auction optimization
-- These indexes improve query performance for lead routing and buyer response lookups

-- Index for campaign lookups by token (used in carina pre-checks)
CREATE INDEX IF NOT EXISTS idx_campaigns_token ON campaigns(campaign_token) WHERE deleted_at IS NULL;

-- Indexes for buyer_responses bulk queries and lookups
CREATE INDEX IF NOT EXISTS idx_buyer_responses_lead_id ON buyer_responses(lead_id);
CREATE INDEX IF NOT EXISTS idx_buyer_responses_campaign_id ON buyer_responses(campaign_id);

-- Composite index for ping tree campaigns lookup (enabled campaigns per ping tree)
CREATE INDEX IF NOT EXISTS idx_ptc_enabled_lookup ON ping_tree_campaigns(ping_tree_id, enabled) WHERE enabled = true;

-- Note: idx_ptp_routing already exists in 20260120000002_create_ping_tree_publishers.sql
-- This migration adds additional indexes for auction performance
