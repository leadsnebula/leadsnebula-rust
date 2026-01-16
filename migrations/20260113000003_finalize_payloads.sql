-- Finalize payload tables: add JSON payload columns and backfill from legacy tables when present
-- Migration: 20260113000003_finalize_payloads.sql

BEGIN;

-- ensure uuid generator available
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- add JSONB payload column if missing
ALTER TABLE ping_payloads ADD COLUMN IF NOT EXISTS payload JSONB;
ALTER TABLE post_payloads ADD COLUMN IF NOT EXISTS payload JSONB;

-- Backfill only when legacy tables exist and buyer_response column exists (safe for test DBs with minimal schema)
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'pings')
     AND EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'pings' AND column_name = 'buyer_response') THEN
    -- backfill existing ping_payloads from pings.buyer_response where possible
    UPDATE ping_payloads pp
    SET payload = p.buyer_response
    FROM pings p
    WHERE pp.ping_id::text = p.id::text
      AND (pp.payload IS NULL)
      AND p.buyer_response IS NOT NULL;

    -- insert ping_payloads rows for pings that have buyer_response but no payload row
    -- Note: id is BIGSERIAL, so we don't specify it (auto-generated)
    INSERT INTO ping_payloads (ping_id, lead_id, payload, created_at, updated_at)
    SELECT p.id::text, p.lead_id, p.buyer_response, now(), now()
    FROM pings p
    WHERE p.buyer_response IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM ping_payloads pp WHERE pp.ping_id = p.id::text);

    -- finally drop legacy buyer_response column from pings
    ALTER TABLE pings DROP COLUMN IF EXISTS buyer_response;
  END IF;
END$$;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'posts')
     AND EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'posts' AND column_name = 'buyer_response') THEN
    -- backfill existing post_payloads from posts.buyer_response where possible
    UPDATE post_payloads pp
    SET payload = pos.buyer_response
    FROM posts pos
    WHERE pp.post_id::text = pos.id::text
      AND (pp.payload IS NULL)
      AND pos.buyer_response IS NOT NULL;

    -- insert post_payloads rows for posts that have buyer_response but no payload row
    -- Note: id is BIGSERIAL, so we don't specify it (auto-generated)
    INSERT INTO post_payloads (post_id, lead_id, payload, created_at, updated_at)
    SELECT pos.id::text, pos.lead_id, pos.buyer_response, now(), now()
    FROM posts pos
    WHERE pos.buyer_response IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM post_payloads pp WHERE pp.post_id = pos.id::text);

    -- finally drop legacy buyer_response column from posts
    ALTER TABLE posts DROP COLUMN IF EXISTS buyer_response;
  END IF;
END$$;

COMMIT;

-- Safety note: this migration is idempotent and guarded with IF NOT EXISTS/IF EXISTS checks.
