-- Make api_key_encrypted NOT NULL
-- Note: Existing NULL values will need to be regenerated via the regenerate API key endpoint
-- For now, we'll set them to empty string as a temporary measure
-- Publishers with empty api_key_encrypted will need to regenerate their keys
UPDATE publishers 
SET api_key_encrypted = '' 
WHERE api_key_encrypted IS NULL;

-- Now make the column NOT NULL
ALTER TABLE publishers
  ALTER COLUMN api_key_encrypted SET NOT NULL;
