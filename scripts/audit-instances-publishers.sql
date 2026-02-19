-- Audit instances and publishers for prod/dev normalization.
-- Run: from rust/  export $(grep -v '^#' .env.local | grep -v '^$' | xargs); psql "$DATABASE_URL" -f scripts/audit-instances-publishers.sql

\echo '=== All instances (id, name, instance_user_id) ==='
SELECT i.id, i.name, i.instance_user_id, i.deleted_at,
       u.email AS owner_email
FROM instances i
LEFT JOIN instance_users u ON u.id = i.instance_user_id
ORDER BY i.name;

\echo ''
\echo '=== All publishers (id, name, instance_id, instance name) ==='
SELECT p.id, p.name, p.instance_id, i.name AS instance_name, p.deleted_at
FROM publishers p
LEFT JOIN instances i ON i.id = p.instance_id
WHERE p.deleted_at IS NULL
ORDER BY i.name NULLS LAST, p.name;

\echo ''
\echo '=== Target publishers (Only Solar, Only Solar Dev, Leads Test) ==='
SELECT p.id, p.name, p.instance_id, i.name AS instance_name
FROM publishers p
LEFT JOIN instances i ON i.id = p.instance_id AND i.deleted_at IS NULL
WHERE p.id IN (
  '0d2a06f2-af57-40c8-b3c7-b61fc7621de6'::uuid,
  '835d1d9f-201f-47fb-a4ae-bf95dbfe72ad'::uuid,
  'a1b2c3d4-e5f6-4780-a123-456789abcdef'::uuid
)
   OR p.name IN ('Only Solar', 'Only Solar Dev', 'Leads Test');

\echo ''
\echo '=== Instance 39c83a64 (prod-only?) ==='
SELECT i.id, i.name, i.instance_user_id
FROM instances i
WHERE i.id = '39c83a64-2db7-4787-bb34-26e0ea8199c3'::uuid;

SELECT p.id, p.name, p.instance_id
FROM publishers p
WHERE p.instance_id = '39c83a64-2db7-4787-bb34-26e0ea8199c3'::uuid AND p.deleted_at IS NULL;
