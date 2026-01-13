-- Ensure legacy payload columns are removed from pings and posts tables
-- Migration: 20260114000001_ensure_legacy_payload_columns_removed.sql
-- 
-- This migration ensures that legacy columns (buyer_response, payload) are removed
-- from pings and posts tables, as payloads are now stored in dedicated ping_payloads
-- and post_payloads tables.

BEGIN;

-- Remove buyer_response from pings if it still exists
-- (This was the legacy column that stored buyer response JSON)
ALTER TABLE pings DROP COLUMN IF EXISTS buyer_response;

-- Remove payload from pings if it still exists
-- (This was another legacy column that may have existed)
ALTER TABLE pings DROP COLUMN IF EXISTS payload;

-- Remove buyer_response from posts if it still exists
ALTER TABLE posts DROP COLUMN IF EXISTS buyer_response;

-- Remove payload from posts if it still exists
ALTER TABLE posts DROP COLUMN IF EXISTS payload;

-- Note: There was never a "ping_payloads" column in the pings table.
-- The legacy column was "buyer_response" which has been migrated to the
-- separate ping_payloads table.

COMMIT;
