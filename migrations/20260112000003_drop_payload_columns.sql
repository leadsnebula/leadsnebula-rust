-- Drop payload columns from `pings` and `posts` after migration to payload tables
-- Migration: 20260112000003_drop_payload_columns.sql

-- This migration will abort if any non-null payload remains in `pings` or `posts`
-- that was not copied into the corresponding `ping_payloads`/`post_payloads`.

DO $$
BEGIN
  -- Drop payload from pings if safe
  IF EXISTS(
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = 'pings' AND column_name = 'payload'
  ) THEN
    IF EXISTS(
      SELECT 1 FROM pings p
      WHERE p.payload IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM ping_payloads pp WHERE pp.ping_id = p.id)
    ) THEN
      RAISE EXCEPTION 'Aborting: some pings.payload rows were not migrated to ping_payloads';
    END IF;

    ALTER TABLE pings DROP COLUMN IF EXISTS payload;
  END IF;

  -- Drop payload from posts if safe
  IF EXISTS(
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = 'posts' AND column_name = 'payload'
  ) THEN
    IF EXISTS(
      SELECT 1 FROM posts p
      WHERE p.payload IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM post_payloads pp WHERE pp.post_id = p.id)
    ) THEN
      RAISE EXCEPTION 'Aborting: some posts.payload rows were not migrated to post_payloads';
    END IF;

    ALTER TABLE posts DROP COLUMN IF EXISTS payload;
  END IF;

END$$;

-- Note: If you intentionally want to force-drop the columns, run the ALTER TABLE statements
-- manually after confirming migration counts and backups.
