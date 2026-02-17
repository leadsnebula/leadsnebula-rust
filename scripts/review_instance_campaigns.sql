-- Review campaign setup for instance 147b486c-dd0f-41bd-a082-ebb50599481b
-- Run: psql "$DATABASE_URL" -f scripts/review_instance_campaigns.sql

\set instance_id '147b486c-dd0f-41bd-a082-ebb50599481b'

\echo '=== Instance ==='
SELECT id, name, payment_status, instance_user_id, created_at
FROM instances
WHERE id = :'instance_id'::uuid AND deleted_at IS NULL;

\echo ''
\echo '=== Publishers (this instance) ==='
SELECT id, name, email, status, instance_id, deleted_at
FROM publishers
WHERE instance_id = :'instance_id'::uuid AND deleted_at IS NULL
ORDER BY name;

\echo ''
\echo '=== Campaigns (this instance) ==='
SELECT c.id, c.name, c.vertical, c.status, c.campaign_token, c.deleted_at,
       c.buyer_id, b.name AS buyer_name,
       c.publisher_id, p.name AS publisher_name
FROM campaigns c
LEFT JOIN buyers b ON b.id = c.buyer_id AND b.deleted_at IS NULL
LEFT JOIN publishers p ON p.id = c.publisher_id AND p.deleted_at IS NULL
WHERE c.instance_id = :'instance_id'::uuid AND c.deleted_at IS NULL
ORDER BY c.vertical, c.name;

\echo ''
\echo '=== Publisher verticals (this instance) ==='
SELECT p.id AS publisher_id, p.name AS publisher_name, v.slug AS vertical_slug, v.name AS vertical_name
FROM publisher_verticals pv
JOIN publishers p ON p.id = pv.publisher_id AND p.deleted_at IS NULL
JOIN verticals v ON v.id = pv.vertical_id AND v.is_active = true
WHERE p.instance_id = :'instance_id'::uuid
ORDER BY p.name, v.slug;

\echo ''
\echo '=== Verticals (active) ==='
SELECT id, slug, name, is_active FROM verticals WHERE is_active = true ORDER BY slug;

\echo ''
\echo '=== Summary: campaigns per vertical for this instance ==='
SELECT c.vertical, COUNT(*) AS campaign_count
FROM campaigns c
WHERE c.instance_id = :'instance_id'::uuid AND c.deleted_at IS NULL
GROUP BY c.vertical
ORDER BY c.vertical;
