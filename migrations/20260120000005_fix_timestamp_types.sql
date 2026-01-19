-- Fix timestamp types in ping_tree_publishers table
-- Change TIMESTAMP to TIMESTAMPTZ to match Rust chrono::DateTime<Utc>

ALTER TABLE ping_tree_publishers 
  ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at AT TIME ZONE 'UTC',
  ALTER COLUMN updated_at TYPE TIMESTAMPTZ USING updated_at AT TIME ZONE 'UTC';
