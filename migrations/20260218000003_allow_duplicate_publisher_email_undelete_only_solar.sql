-- Allow multiple publishers to share the same email (e.g. Only Solar and Only Solar Dev).
-- Then undelete Only Solar in Leads Nebula instance so both pubs are active.

-- 1. Drop the partial unique index that required distinct email for non-deleted publishers
DROP INDEX IF EXISTS publishers_email_unique_not_deleted;

-- 2. Undelete Only Solar (147b486c) so both Only Solar and Only Solar Dev are active
UPDATE publishers
SET deleted_at = NULL,
    updated_at = NOW()
WHERE id = '0d2a06f2-af57-40c8-b3c7-b61fc7621de6'::uuid;
