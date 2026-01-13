-- Migrate payload JSON from existing `pings.payload` and `posts.payload`
-- Migration: 20260112000002_migrate_payloads.sql

-- NOTE: This migration is safe to run even if the source `payload` column
-- no longer exists. It will only copy rows when the column is present.

DO $$
BEGIN
  -- Copy ping payloads if the column exists
  IF EXISTS(
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = 'pings' AND column_name = 'payload'
  ) THEN
    INSERT INTO ping_payloads (lead_id, ping_id, payload, created_at, updated_at)
    SELECT p.lead_id, p.id, p.payload, p.created_at, p.updated_at
    FROM pings p
    WHERE p.payload IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM ping_payloads pp WHERE pp.ping_id = p.id);
  END IF;

  -- Copy post payloads if the column exists
  IF EXISTS(
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = 'posts' AND column_name = 'payload'
  ) THEN
    INSERT INTO post_payloads (lead_id, post_id, payload, created_at, updated_at)
    SELECT p.lead_id, p.id, p.payload, p.created_at, p.updated_at
    FROM posts p
    WHERE p.payload IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM post_payloads pp WHERE pp.post_id = p.id);
  END IF;

END$$;

-- After running this migration, verify counts and backups before dropping
-- the original `payload` columns. A follow-up migration will remove those
-- columns once we're confident migration is complete.
