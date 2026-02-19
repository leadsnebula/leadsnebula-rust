-- Why leads from dev.only.solar (publisher 835d1d9f Only Solar Dev) don't show for boris@ (Leads Nebula 147b486c).
-- Run against the DB that the API uses when receiving submissions AND the DB the dashboard uses when boris@ views leads.
-- If those are different DBs, run this on BOTH and compare.

\echo '=== 1. Publisher Only Solar Dev (835d1d9f): instance_id must be 147b486c for boris@ to see leads ==='
SELECT id, name, instance_id, deleted_at
FROM publishers
WHERE id = '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid;

\echo ''
\echo '=== 2. Instance 147b486c owner (must be boris@) ==='
SELECT i.id, i.name, i.instance_user_id, u.email AS owner_email
FROM instances i
LEFT JOIN instance_users u ON u.id = i.instance_user_id
WHERE i.id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid AND i.deleted_at IS NULL;

\echo ''
\echo '=== 3. Recent leads for publisher 835d1d9f (Only Solar Dev) - last 10 ==='
SELECT l.uuid, l.lead_id, l.status::text, l.publisher_id, l.created_at AT TIME ZONE 'UTC' AS created_utc
FROM leads l
WHERE l.publisher_id = '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid
ORDER BY l.created_at DESC
LIMIT 10;

\echo ''
\echo '=== 4. Count of leads for Only Solar Dev in this DB ==='
SELECT COUNT(*) AS lead_count
FROM leads
WHERE publisher_id = '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid;

\echo ''
\echo '=== 5. list_leads filter: leads that WOULD show for boris@ (instance 147b486c) - last 5 ==='
SELECT l.uuid, l.lead_id, l.status::text, p.name AS publisher_name, l.created_at AT TIME ZONE 'UTC' AS created_utc
FROM leads l
JOIN publishers pub ON pub.id = l.publisher_id AND pub.instance_id = '147b486c-dd0f-41bd-a082-ebb50599481b'::uuid AND pub.deleted_at IS NULL
LEFT JOIN publishers p ON p.id = l.publisher_id
ORDER BY l.created_at DESC
LIMIT 5;
