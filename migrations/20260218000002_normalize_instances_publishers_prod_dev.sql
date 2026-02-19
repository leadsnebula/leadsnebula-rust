-- Normalize instances and publishers between prod and dev DB branches.
-- Target structure:
--   Leads Nebula Instance (147b486c): publishers Only Solar (0d2a06f2), Only Solar Dev (835d1d9f)
--   Leads Test Instance (c3d4e5f6): publisher Leads Test (a1b2c3d4)
-- Idempotent: safe to run on both branches. Restores Only Solar if soft-deleted; moves
-- campaigns, buyers, and ping_trees to match publisher instance assignments.
-- Run on both prod and dev after approval.

-- 1. Assign publishers to correct instances (do not clear deleted_at for Only Solar:
--    it shares email info@only.solar with Only Solar Dev; partial unique on email would fail)
UPDATE publishers
SET instance_id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid,
    updated_at = NOW()
WHERE id IN (
  '0d2a06f2-af57-40c8-b3c7-b61fc7621de6'::uuid,
  '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid
);

UPDATE publishers
SET instance_id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
    updated_at = NOW()
WHERE id = 'a1b2c3d4-e5f6-4780-a123-456789abcdef'::uuid;

-- 2. Move campaigns to match publisher instance (Leads Nebula pubs -> 147b486c)
UPDATE campaigns
SET instance_id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid,
    updated_at = NOW()
WHERE publisher_id IN (
  '0d2a06f2-af57-40c8-b3c7-b61fc7621de6'::uuid,
  '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid
)
  AND (instance_id IS DISTINCT FROM '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid OR instance_id IS NULL);

-- 3. Move campaigns for Leads Test publisher to Leads Test Instance
UPDATE campaigns
SET instance_id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
    updated_at = NOW()
WHERE publisher_id = 'a1b2c3d4-e5f6-4780-a123-456789abcdef'::uuid
  AND (instance_id IS DISTINCT FROM 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid OR instance_id IS NULL);

-- 4. Move buyers that are referenced by campaigns now on 147b486c to that instance
UPDATE buyers
SET instance_id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid,
    updated_at = NOW()
WHERE id IN (
  SELECT DISTINCT c.buyer_id
  FROM campaigns c
  WHERE c.instance_id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
    AND c.deleted_at IS NULL
)
  AND (instance_id IS DISTINCT FROM '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid OR instance_id IS NULL);

-- 5. Move buyers referenced by campaigns on Leads Test Instance
UPDATE buyers
SET instance_id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
    updated_at = NOW()
WHERE id IN (
  SELECT DISTINCT c.buyer_id
  FROM campaigns c
  WHERE c.instance_id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid
    AND c.deleted_at IS NULL
)
  AND (instance_id IS DISTINCT FROM 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid OR instance_id IS NULL);

-- 6. Move ping_trees that have Only Solar / Only Solar Dev (via ping_tree_publishers) to Leads Nebula Instance
UPDATE ping_trees
SET instance_id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid,
    updated_at = NOW()
WHERE id IN (
  SELECT DISTINCT ptp.ping_tree_id
  FROM ping_tree_publishers ptp
  WHERE ptp.publisher_id IN (
    '0d2a06f2-af57-40c8-b3c7-b61fc7621de6'::uuid,
    '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid
  )
)
  AND (instance_id IS DISTINCT FROM '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid OR instance_id IS NULL);

-- 7. Move ping_trees that have Leads Test publisher to Leads Test Instance
UPDATE ping_trees
SET instance_id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid,
    updated_at = NOW()
WHERE id IN (
  SELECT DISTINCT ptp.ping_tree_id
  FROM ping_tree_publishers ptp
  WHERE ptp.publisher_id = 'a1b2c3d4-e5f6-4780-a123-456789abcdef'::uuid
)
  AND (instance_id IS DISTINCT FROM 'c3d4e5f6-a7b8-4780-c345-678901abcdef'::uuid OR instance_id IS NULL);

-- 8. Soft-delete prod-only instance 39c83a64 if present (so prod and dev have identical canonical instances)
UPDATE instances
SET deleted_at = NOW(),
    updated_at = NOW()
WHERE id = '39c83a64-2db7-4787-bb34-26e0ea8199c3'::uuid
  AND deleted_at IS NULL;
