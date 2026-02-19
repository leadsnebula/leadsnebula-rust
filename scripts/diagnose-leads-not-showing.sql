-- =============================================================================
-- DIAGNOSTIC: Why leads (especially error/validation) are not in DB or not shown
-- Run:  psql "$DATABASE_URL" -f scripts/diagnose-leads-not-showing.sql
-- Or:  from rust/ run  psql (your connection string) -f scripts/diagnose-leads-not-showing.sql
--
-- ROOT CAUSE CHECK (section 1–2):
-- If migration 20260217000001 is NOT in _sqlx_migrations, then leads.buyer_id
-- and leads.campaign_id are still NOT NULL. The app inserts error leads with
-- NULL for those when the instance has no campaign, so the INSERT fails and
-- the lead is never written (only a warning is logged). Fix: run migrations.
-- =============================================================================

\echo '=== 1. Applied migrations (check if 20260217000001 is present) ==='
SELECT version, description, success
FROM _sqlx_migrations
ORDER BY version;

\echo ''
\echo '=== 2. leads table: nullable state of buyer_id, campaign_id, post_id ==='
SELECT column_name, is_nullable, data_type
FROM information_schema.columns
WHERE table_schema = 'public' AND table_name = 'leads'
  AND column_name IN ('buyer_id', 'campaign_id', 'post_id', 'publisher_id', 'vertical_id')
ORDER BY ordinal_position;

\echo ''
\echo '=== 3. Look for specific lead UUID (if not found, insert never happened or wrong DB) ==='
SELECT uuid, lead_id, status::text, publisher_id, vertical_id, buyer_id, campaign_id,
       submitted_at, created_at,
       vertical_data::text
FROM leads
WHERE uuid = '16fd1a89-7087-479f-84a6-bb2e3597df9a';

\echo ''
\echo '=== 4. Any leads in pings for that UUID? (persist writes lead then ping then ping_payloads) ==='
SELECT id, ping_id, lead_id, state, created_at
FROM pings
WHERE lead_id = '16fd1a89-7087-479f-84a6-bb2e3597df9a';

\echo ''
\echo '=== 5. Recent error-status leads (last 20) ==='
SELECT l.uuid, l.lead_id, l.status::text, l.publisher_id, l.created_at,
       p.name AS publisher_name,
       pub.instance_id
FROM leads l
LEFT JOIN publishers p ON p.id = l.publisher_id AND p.deleted_at IS NULL
LEFT JOIN publishers pub ON pub.id = l.publisher_id
WHERE l.status::text = 'error'
ORDER BY l.created_at DESC
LIMIT 20;

\echo ''
\echo '=== 6. All leads created around 2026-12-17 19:06 PST (07:06 UTC next day) ==='
SELECT uuid, lead_id, status::text, publisher_id, created_at AT TIME ZONE 'UTC' AS created_utc
FROM leads
WHERE created_at >= '2026-12-17 19:06:00-08'
  AND created_at <  '2026-12-17 20:00:00-08'
ORDER BY created_at DESC;

\echo ''
\echo '=== 7. Publisher -> instance chain (list_leads only shows leads whose publisher.instance_id = your instance) ==='
SELECT p.id AS publisher_id, p.name, p.instance_id, p.deleted_at,
       i.id AS instance_id, i.name AS instance_name
FROM publishers p
LEFT JOIN instances i ON i.id = p.instance_id AND i.deleted_at IS NULL
WHERE p.deleted_at IS NULL
ORDER BY p.created_at DESC
LIMIT 20;

\echo ''
\echo '=== 8. Instance -> instance_user (dashboard user must own an instance to see its leads) ==='
SELECT i.id AS instance_id, i.name, i.instance_user_id,
       u.email AS instance_owner_email
FROM instances i
LEFT JOIN instance_users u ON u.id = i.instance_user_id
WHERE i.deleted_at IS NULL
ORDER BY i.id;

\echo ''
\echo '=== 9. Campaigns per instance (persist_failed_lead needs a campaign unless migration allows NULL) ==='
SELECT c.instance_id, COUNT(*) AS campaign_count
FROM campaigns c
WHERE c.deleted_at IS NULL
GROUP BY c.instance_id
ORDER BY campaign_count DESC
LIMIT 20;
