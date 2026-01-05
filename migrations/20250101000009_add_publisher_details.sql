-- Add additional fields to publishers table for frontend requirements
ALTER TABLE publishers
  ADD COLUMN IF NOT EXISTS representative_first_name VARCHAR(255),
  ADD COLUMN IF NOT EXISTS representative_last_name VARCHAR(255),
  ADD COLUMN IF NOT EXISTS address_street TEXT,
  ADD COLUMN IF NOT EXISTS address_city VARCHAR(255),
  ADD COLUMN IF NOT EXISTS address_state VARCHAR(10),
  ADD COLUMN IF NOT EXISTS address_zip VARCHAR(20),
  ADD COLUMN IF NOT EXISTS timezone VARCHAR(100),
  ADD COLUMN IF NOT EXISTS ein_tin VARCHAR(50);

-- Add index on ein_tin for faster lookups
CREATE INDEX IF NOT EXISTS idx_publishers_ein_tin ON publishers(ein_tin);
