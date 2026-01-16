-- Create payload tables for pings and posts
-- Migration: 20260112000001_create_payload_tables.sql

-- ping_payloads: stores JSON payloads originally kept in `pings.payload`
-- Drop table if it exists but is missing required columns (from partial migration)
DO $$ 
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'ping_payloads'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'ping_payloads' AND column_name = 'payload'
    ) THEN
        DROP TABLE IF EXISTS ping_payloads CASCADE;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS ping_payloads (
  id BIGSERIAL PRIMARY KEY,
  lead_id UUID REFERENCES leads(uuid) ON DELETE CASCADE,
  ping_id VARCHAR(255),
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Ensure payload column exists
DO $$ 
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'ping_payloads' AND column_name = 'payload'
    ) THEN
        ALTER TABLE ping_payloads ADD COLUMN payload JSONB NOT NULL DEFAULT '{}'::jsonb;
        ALTER TABLE ping_payloads ALTER COLUMN payload DROP DEFAULT;
    END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_ping_payloads_ping_id_unique ON ping_payloads (ping_id) WHERE ping_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ping_payloads_lead_id ON ping_payloads (lead_id);
CREATE INDEX IF NOT EXISTS idx_ping_payloads_created_at ON ping_payloads (created_at);
CREATE INDEX IF NOT EXISTS idx_ping_payloads_payload_gin ON ping_payloads USING GIN (payload);

-- post_payloads: stores JSON payloads originally kept in `posts.payload`
-- Drop table if it exists but is missing required columns (from partial migration)
DO $$ 
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'post_payloads'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'post_payloads' AND column_name = 'payload'
    ) THEN
        DROP TABLE IF EXISTS post_payloads CASCADE;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS post_payloads (
  id BIGSERIAL PRIMARY KEY,
  lead_id UUID REFERENCES leads(uuid) ON DELETE CASCADE,
  post_id VARCHAR(255),
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Ensure payload column exists
DO $$ 
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'post_payloads' AND column_name = 'payload'
    ) THEN
        ALTER TABLE post_payloads ADD COLUMN payload JSONB NOT NULL DEFAULT '{}'::jsonb;
        ALTER TABLE post_payloads ALTER COLUMN payload DROP DEFAULT;
    END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_post_payloads_post_id_unique ON post_payloads (post_id) WHERE post_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_post_payloads_lead_id ON post_payloads (lead_id);
CREATE INDEX IF NOT EXISTS idx_post_payloads_created_at ON post_payloads (created_at);
CREATE INDEX IF NOT EXISTS idx_post_payloads_payload_gin ON post_payloads USING GIN (payload);

-- Notes:
-- 1) This migration only creates the tables and indexes. Data migration from `pings.payload` and `posts.payload`
--    should be done in a follow-up migration that copies rows into these tables inside a transaction.
-- 2) Use the unique index on ping_id/post_id to ensure one-to-one mapping where applicable.
