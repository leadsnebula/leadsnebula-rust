-- Cleanup development DB: keep only 2 instances and 2 instance_users, fix links and logins.
-- Run ONLY on development database. Use: psql $DATABASE_URL -f scripts/cleanup-dev-instances-and-users.sql
--
-- Keeps:
--   Instance c3d4e5f6-a7b8-4780-c345-678901abcdef (API Lead Test / Leads Test Instance) -> user b2c3d4e5 -> info@leadsnebula.com
--   Instance 147b486c-dd0f-41bd-a082-ebb50599481b (Leads Nebula) -> user d6bf3245 -> boris@leadsnebula.com
-- After running, set passwords with:
--   cd rust && cargo run -p leadsnebula-utils --bin update-password -- --email info@leadsnebula.com --password '<API_LEAD_TEST_PASSWORD>'
--   cd rust && cargo run -p leadsnebula-utils --bin update-password -- --email boris@leadsnebula.com --password '<LEADS_NEBULA_PASSWORD>'

BEGIN;

-- 1. Delete dependent data for instances we are about to remove (not the two we keep)
DELETE FROM leads
WHERE publisher_id IN (
  SELECT id FROM publishers
  WHERE instance_id NOT IN (
    'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
    '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
  )
);

DELETE FROM ping_tree_campaigns
WHERE ping_tree_id IN (
  SELECT id FROM ping_trees
  WHERE instance_id NOT IN (
    'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
    '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
  )
)
OR campaign_id IN (
  SELECT id FROM campaigns
  WHERE instance_id NOT IN (
    'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
    '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
  )
);

DELETE FROM ping_trees
WHERE instance_id NOT IN (
  'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
  '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
);

DELETE FROM campaigns
WHERE instance_id NOT IN (
  'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
  '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
);

DELETE FROM buyers
WHERE instance_id NOT IN (
  'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
  '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
);

DELETE FROM publishers
WHERE instance_id NOT IN (
  'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
  '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
);

-- audit_logs if table exists
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'audit_logs') THEN
    DELETE FROM audit_logs
    WHERE instance_id IS NOT NULL
      AND instance_id NOT IN (
        'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
        '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
      );
  END IF;
END $$;

-- 2. Link instances to the two users we keep (API Lead Test = b2c3d4e5, Leads Nebula = d6bf3245)
UPDATE instances
SET instance_user_id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef'::uuid
WHERE id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid;

UPDATE instances
SET instance_user_id = 'd6bf3245-833d-4176-954b-edfa08e5edb5'::uuid
WHERE id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid;

-- 3. Delete all instances except the two
DELETE FROM instances
WHERE id NOT IN (
  'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
  '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
);

-- 4. Delete all instance_users except the two (cascades will clean webauthn_credentials, etc.)
DELETE FROM instance_users
WHERE id NOT IN (
  'b2c3d4e5-f6a7-4780-b234-567890abcdef'::uuid,
  'd6bf3245-833d-4176-954b-edfa08e5edb5'::uuid
);

-- 5. Set correct emails: API Lead Test instance = info@, Leads Nebula instance = boris@ (passwords unchanged)
-- Order matters if both had the same email: update Leads Nebula user first, then API Lead Test user
UPDATE instance_users
SET email = 'boris@leadsnebula.com',
    confirmed_at = COALESCE(confirmed_at, NOW()),
    updated_at = NOW()
WHERE id = 'd6bf3245-833d-4176-954b-edfa08e5edb5'::uuid;

UPDATE instance_users
SET email = 'info@leadsnebula.com',
    confirmed_at = COALESCE(confirmed_at, NOW()),
    updated_at = NOW()
WHERE id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef'::uuid;

-- Summary
SELECT 'instances' AS tbl, COUNT(*) AS n FROM instances
UNION ALL SELECT 'instance_users', COUNT(*) FROM instance_users;

COMMIT;
