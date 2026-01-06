-- Allow emails from deleted publishers to be reused
-- Remove the UNIQUE constraint on email and replace with a partial unique index
-- that only applies to non-deleted publishers

-- Drop the existing UNIQUE constraint on email
ALTER TABLE publishers DROP CONSTRAINT IF EXISTS publishers_email_key;

-- Create a partial unique index that only applies to non-deleted publishers
-- This allows deleted publishers to have the same email, but prevents
-- multiple active/inactive publishers from having the same email
CREATE UNIQUE INDEX IF NOT EXISTS publishers_email_unique_not_deleted 
ON publishers(email) 
WHERE deleted_at IS NULL;
