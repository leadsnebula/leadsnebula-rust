-- Add encrypted payload columns to ping_payloads and post_payloads
-- Migration: 20260113000001_add_encrypted_payload_columns.sql

ALTER TABLE ping_payloads
  ADD COLUMN IF NOT EXISTS request_payload_encrypted TEXT,
  ADD COLUMN IF NOT EXISTS response_payload_encrypted TEXT;

ALTER TABLE post_payloads
  ADD COLUMN IF NOT EXISTS request_payload_encrypted TEXT,
  ADD COLUMN IF NOT EXISTS response_payload_encrypted TEXT;

-- Backfill strategy is to be implemented separately; new code will write encrypted columns going forward.
