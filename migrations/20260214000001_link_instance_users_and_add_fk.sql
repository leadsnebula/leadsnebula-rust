-- Assign instance users to instances and add foreign key.
-- Requirements:
--   boris@leadsnebula.com -> Leads Nebula instance (147b486c-dd0f-41bd-a082-ebb50599481b)
--   User d6bf3245 (info@leadsnebula.com) -> API Leads Test Instance (c3d4e5f6-a7b8-4780-c345-678901abcdef)
-- Uses b2c3d4e5 as the second user (boris@) when present; idempotent.
-- Order: use temp emails to avoid unique constraint when swapping emails between users.
-- Fresh-DB safe: on a DB where only 20260211000001 ran (b2c3d4e5 + instance c3d4e5f6), we do not
-- assign instances to d6bf3245 (which does not exist); we only add the FK so migrations succeed.

-- 1. Free both target emails by moving current holders to temp addresses (only existing rows)
UPDATE instance_users SET email = 'leadsnebula-instance-user-d6bf3245@temp.local', updated_at = NOW()
WHERE id = 'd6bf3245-833d-4176-954b-edfa08e5edb5';
UPDATE instance_users SET email = 'leadsnebula-instance-user-b2c3d4e5@temp.local', updated_at = NOW()
WHERE id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef';

-- 2. Assign final emails and link instances (only when target users/instances exist)
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM instance_users WHERE id = 'd6bf3245-833d-4176-954b-edfa08e5edb5') THEN
    UPDATE instance_users SET email = 'info@leadsnebula.com', updated_at = NOW()
    WHERE id = 'd6bf3245-833d-4176-954b-edfa08e5edb5';
    UPDATE instances SET instance_user_id = 'd6bf3245-833d-4176-954b-edfa08e5edb5', updated_at = NOW()
    WHERE id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef';
  ELSE
    -- Fresh DB: only b2c3d4e5 and instance c3d4e5f6 exist; keep instance owned by b2c3d4e5, set email to info@
    UPDATE instance_users SET email = 'info@leadsnebula.com', updated_at = NOW()
    WHERE id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef';
  END IF;

  -- Only assign boris@ and instance 147b486c when that instance exists (two-instance dev setup)
  IF EXISTS (SELECT 1 FROM instances WHERE id = '147b486c-dd0f-41bd-a082-ebb50599481b') THEN
    IF EXISTS (SELECT 1 FROM instance_users WHERE id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef') THEN
      UPDATE instance_users SET email = 'boris@leadsnebula.com', updated_at = NOW() WHERE id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef';
      UPDATE instances SET instance_user_id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef', updated_at = NOW() WHERE id = '147b486c-dd0f-41bd-a082-ebb50599481b';
    ELSE
      UPDATE instance_users
      SET email = 'boris@leadsnebula.com', updated_at = NOW()
      WHERE id = (SELECT instance_user_id FROM instances WHERE id = '147b486c-dd0f-41bd-a082-ebb50599481b' LIMIT 1)
        AND id != 'd6bf3245-833d-4176-954b-edfa08e5edb5';
    END IF;
  END IF;
END $$;

-- 3. Add foreign key from instances.instance_user_id to instance_users.id (idempotent)
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conrelid = 'instances'::regclass AND conname = 'fk_instances_instance_user_id'
  ) THEN
    ALTER TABLE instances
    ADD CONSTRAINT fk_instances_instance_user_id
    FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE SET NULL;
  END IF;
END $$;
