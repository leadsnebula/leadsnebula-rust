-- Create buyer_responses table to persist every buyer reply for audit
-- Migration: 20260113000004_create_buyer_responses.sql

CREATE TABLE IF NOT EXISTS buyer_responses (
  id BIGSERIAL PRIMARY KEY,
  lead_id UUID REFERENCES leads(uuid) ON DELETE CASCADE,
  ping_id VARCHAR(255),
  post_id VARCHAR(255),
  buyer_id UUID,
  campaign_id UUID,
  payload JSONB NOT NULL,
  response_payload_encrypted TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_buyer_responses_lead_id ON buyer_responses (lead_id);
CREATE INDEX IF NOT EXISTS idx_buyer_responses_ping_id ON buyer_responses (ping_id);
CREATE INDEX IF NOT EXISTS idx_buyer_responses_post_id ON buyer_responses (post_id);
CREATE INDEX IF NOT EXISTS idx_buyer_responses_buyer_id ON buyer_responses (buyer_id);
CREATE INDEX IF NOT EXISTS idx_buyer_responses_created_at ON buyer_responses (created_at);
