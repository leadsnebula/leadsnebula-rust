-- Add missing buyer fields to match Ruby schema
ALTER TABLE buyers
  ADD COLUMN IF NOT EXISTS contact_info JSONB DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS ein_tin VARCHAR(50),
  ADD COLUMN IF NOT EXISTS address_street TEXT,
  ADD COLUMN IF NOT EXISTS address_city VARCHAR(255),
  ADD COLUMN IF NOT EXISTS address_state VARCHAR(10),
  ADD COLUMN IF NOT EXISTS address_zip VARCHAR(20),
  ADD COLUMN IF NOT EXISTS email_address VARCHAR(255),
  ADD COLUMN IF NOT EXISTS representative_first_name VARCHAR(255),
  ADD COLUMN IF NOT EXISTS representative_last_name VARCHAR(255),
  ADD COLUMN IF NOT EXISTS documents JSONB DEFAULT '[]',
  ADD COLUMN IF NOT EXISTS post_type VARCHAR(20) DEFAULT 'full_post' NOT NULL,
  ADD COLUMN IF NOT EXISTS buyer_type VARCHAR(20);

-- Add indexes
CREATE INDEX IF NOT EXISTS idx_buyers_ein_tin ON buyers(ein_tin);
CREATE INDEX IF NOT EXISTS idx_buyers_email_address ON buyers(email_address);
CREATE INDEX IF NOT EXISTS idx_buyers_buyer_type ON buyers(buyer_type);
CREATE INDEX IF NOT EXISTS idx_buyers_post_type ON buyers(post_type);
CREATE INDEX IF NOT EXISTS idx_buyers_buyer_integration_id ON buyers(buyer_integration_id);
