-- Add encrypted API key column to publishers table
-- This allows admins to retrieve full API keys after creation
ALTER TABLE publishers
  ADD COLUMN IF NOT EXISTS api_key_encrypted TEXT;

-- Add index for faster lookups (though we'll primarily query by id)
-- Note: We can't index encrypted TEXT efficiently, so this is optional
-- CREATE INDEX IF NOT EXISTS idx_publishers_api_key_encrypted ON publishers(api_key_encrypted) WHERE api_key_encrypted IS NOT NULL;
