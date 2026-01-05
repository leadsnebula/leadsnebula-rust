-- Remove unique constraint on buyer name to allow duplicate names
ALTER TABLE buyers DROP CONSTRAINT IF EXISTS idx_buyers_instance_name;
