-- Fix leads stuck in 'processing' status that should be 'sold'
-- Migration: 20260126000001_fix_stuck_processing_leads.sql
--
-- This migration fixes historical leads that have campaign_id and buyer_id set
-- (indicating they were sold) but are still marked as 'processing' status.
-- This can happen when the status update logic didn't properly mark them as sold.
--
-- Safe to run multiple times (idempotent)

-- Update leads that have campaign_id and buyer_id set but are still 'processing'
-- Set status to 'sold' and sold_at timestamp if not already set
UPDATE leads
SET 
    status = 'sold',
    sold_at = COALESCE(sold_at, updated_at, NOW()),
    updated_at = NOW()
WHERE 
    status = 'processing'
    AND campaign_id IS NOT NULL
    AND buyer_id IS NOT NULL
    AND (sold_at IS NULL OR status != 'sold');

-- Log the number of leads fixed
DO $$
DECLARE
    fixed_count INTEGER;
BEGIN
    GET DIAGNOSTICS fixed_count = ROW_COUNT;
    RAISE NOTICE 'Fixed % leads from processing to sold status', fixed_count;
END $$;
