-- Enforce NOT NULL constraints on critical leads columns
-- Migration: 20260112000004_enforce_leads_not_nulls.sql

-- Strategy:
-- 1) Fill `submitted_at` with `created_at` where missing.
-- 2) Count nullable critical columns; if any rows would violate constraints,
--    abort with an error listing counts so operator can resolve manually.
-- 3) If no violating rows, apply NOT NULL constraints.

-- Step 1: populate submitted_at where null
UPDATE leads SET submitted_at = created_at WHERE submitted_at IS NULL;

-- Step 2: Check for remaining nulls in critical columns
DO $$
DECLARE
  missing_buyer bigint;
  missing_publisher bigint;
  missing_campaign bigint;
  missing_post bigint;
BEGIN
  SELECT COUNT(*) INTO missing_buyer FROM leads WHERE buyer_id IS NULL;
  SELECT COUNT(*) INTO missing_publisher FROM leads WHERE publisher_id IS NULL;
  SELECT COUNT(*) INTO missing_campaign FROM leads WHERE campaign_id IS NULL;
  SELECT COUNT(*) INTO missing_post FROM leads WHERE post_id IS NULL;

  IF missing_buyer > 0 OR missing_publisher > 0 OR missing_campaign > 0 OR missing_post > 0 THEN
    RAISE EXCEPTION 'Cannot apply NOT NULL constraints - rows missing values: buyer_id=% missing, publisher_id=% missing, campaign_id=% missing, post_id=% missing',
      missing_buyer, missing_publisher, missing_campaign, missing_post;
  END IF;
END$$;

-- Step 3: Apply NOT NULL constraints (these will succeed only if above checks passed)
ALTER TABLE leads ALTER COLUMN submitted_at SET NOT NULL;
ALTER TABLE leads ALTER COLUMN buyer_id SET NOT NULL;
ALTER TABLE leads ALTER COLUMN publisher_id SET NOT NULL;
ALTER TABLE leads ALTER COLUMN campaign_id SET NOT NULL;
ALTER TABLE leads ALTER COLUMN post_id SET NOT NULL;

-- Note: If your dataset legitimately has missing buyer/publisher/campaign/post values,
-- resolve them before running this migration (either by cleaning data or by deciding
-- on appropriate defaults). This migration intentionally aborts with counts to
-- force manual verification.
