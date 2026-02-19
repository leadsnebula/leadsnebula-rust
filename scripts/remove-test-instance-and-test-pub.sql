-- Remove Test Instance (f1c1083d), Test Pub (37b7d110), and test_iu@test.invalid user.
-- These were created by integration tests, not by user request.
-- Only Solar Instance = Leads Nebula, LLC (boris@). Leads Test Instance (info@) = c3d4e5f6-...
--
-- Run only after explicit user approval.
-- Usage: psql "$DATABASE_URL" -f scripts/remove-test-instance-and-test-pub.sql

BEGIN;

-- 1. Pings for leads that belong to Test Pub (delete before leads)
DELETE FROM pings
WHERE lead_id IN (SELECT uuid FROM leads WHERE publisher_id = '37b7d110-c04c-4f44-91f2-b4ce72e4a1f0'::uuid);

-- 2. Leads for Test Pub (CASCADE will remove ping_payloads, post_payloads, buyer_responses)
DELETE FROM leads
WHERE publisher_id = '37b7d110-c04c-4f44-91f2-b4ce72e4a1f0'::uuid;

-- 3. campaign_publishers rows for campaigns in Test Instance
DELETE FROM campaign_publishers
WHERE campaign_id IN (SELECT id FROM campaigns WHERE instance_id = 'f1c1083d-a125-41fc-bd6c-0d511eaf1f92'::uuid);

-- 4. ping_tree_campaigns for ping_trees in Test Instance
DELETE FROM ping_tree_campaigns
WHERE ping_tree_id IN (SELECT id FROM ping_trees WHERE instance_id = 'f1c1083d-a125-41fc-bd6c-0d511eaf1f92'::uuid);

-- 5. ping_tree_publishers for ping_trees in Test Instance
DELETE FROM ping_tree_publishers
WHERE ping_tree_id IN (SELECT id FROM ping_trees WHERE instance_id = 'f1c1083d-a125-41fc-bd6c-0d511eaf1f92'::uuid);

-- 6. Campaigns in Test Instance
DELETE FROM campaigns
WHERE instance_id = 'f1c1083d-a125-41fc-bd6c-0d511eaf1f92'::uuid;

-- 7. Buyers in Test Instance
DELETE FROM buyers
WHERE instance_id = 'f1c1083d-a125-41fc-bd6c-0d511eaf1f92'::uuid;

-- 8. Ping trees in Test Instance
DELETE FROM ping_trees
WHERE instance_id = 'f1c1083d-a125-41fc-bd6c-0d511eaf1f92'::uuid;

-- 9. publisher_verticals for Test Pub
DELETE FROM publisher_verticals
WHERE publisher_id = '37b7d110-c04c-4f44-91f2-b4ce72e4a1f0'::uuid;

-- 10. Test Pub publisher
DELETE FROM publishers
WHERE id = '37b7d110-c04c-4f44-91f2-b4ce72e4a1f0'::uuid;

-- 11. Test Instance
DELETE FROM instances
WHERE id = 'f1c1083d-a125-41fc-bd6c-0d511eaf1f92'::uuid;

-- 12. test_iu@test.invalid user (and any dependent rows: webauthn, otp, etc. may CASCADE)
DELETE FROM instance_users
WHERE id = '7f05d3e1-0462-4d32-a31f-8c947a8f6206'::uuid
  AND email LIKE 'test_iu_%@test.invalid';

COMMIT;

-- Ensure Only Solar publisher (835d1d9f) belongs to Leads Nebula, LLC (147b486c) only
UPDATE publishers
SET instance_id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid
WHERE id = '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid
  AND instance_id IS DISTINCT FROM '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid;
