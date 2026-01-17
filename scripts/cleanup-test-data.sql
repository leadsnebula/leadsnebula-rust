-- Cleanup script to remove all test data from main database
-- WARNING: This will DELETE all test records from publishers, buyers, campaigns, and ping_trees
-- Run this ONLY on your main/dev database, NOT on production
--
-- Usage:
--   psql $DATABASE_URL -f scripts/cleanup-test-data.sql
--   Or: cat scripts/cleanup-test-data.sql | psql $DATABASE_URL

BEGIN;

-- Delete test publishers (those with test-like names or emails)
DELETE FROM publishers
WHERE 
    name LIKE 'Test%' 
    OR name LIKE 'test%'
    OR email LIKE '%@test.%'
    OR email LIKE '%test%@%'
    OR email LIKE '%@example.com'
    OR api_key_prefix LIKE 'pk_test%'
    OR api_key_hash LIKE 'hash_%';

-- Delete test buyers (those with test-like names)
DELETE FROM buyers
WHERE 
    name LIKE 'Test%'
    OR name LIKE 'test%';

-- Delete test campaigns (those with test-like names or associated with test publishers/buyers)
DELETE FROM campaigns
WHERE 
    name LIKE 'Test%'
    OR name LIKE 'test%'
    OR publisher_id IN (SELECT id FROM publishers WHERE name LIKE 'Test%' OR name LIKE 'test%')
    OR buyer_id IN (SELECT id FROM buyers WHERE name LIKE 'Test%' OR name LIKE 'test%');

-- Delete test ping trees (those with test-like names or associated with test publishers)
DELETE FROM ping_trees
WHERE 
    name LIKE 'Test%'
    OR name LIKE 'test%'
    OR publisher_id IN (SELECT id FROM publishers WHERE name LIKE 'Test%' OR name LIKE 'test%');

-- Delete test ping tree campaigns (orphaned after deleting ping trees)
DELETE FROM ping_tree_campaigns
WHERE 
    ping_tree_id NOT IN (SELECT id FROM ping_trees)
    OR campaign_id NOT IN (SELECT id FROM campaigns);

-- Delete test leads (those with test-like data or associated with test publishers)
DELETE FROM leads
WHERE 
    publisher_id IN (SELECT id FROM publishers WHERE name LIKE 'Test%' OR name LIKE 'test%')
    OR event_id LIKE 'evt_%'
    OR session_id LIKE 'sess_%'
    OR post_id LIKE 'post_%'
    OR post_id LIKE 'INPROG_%'
    OR promise_id LIKE 'PROMISE_%';

-- Delete test instance_users (those with test emails)
DELETE FROM instance_users
WHERE 
    email LIKE '%@test.%'
    OR email LIKE '%test%@%'
    OR email LIKE '%@example.com';

-- Delete test instances (orphaned after deleting instance_users)
DELETE FROM instances
WHERE 
    instance_user_id NOT IN (SELECT id FROM instance_users);

-- Show summary
SELECT 
    'publishers' as table_name, 
    COUNT(*) as remaining_count 
FROM publishers
UNION ALL
SELECT 'buyers', COUNT(*) FROM buyers
UNION ALL
SELECT 'campaigns', COUNT(*) FROM campaigns
UNION ALL
SELECT 'ping_trees', COUNT(*) FROM ping_trees
UNION ALL
SELECT 'ping_tree_campaigns', COUNT(*) FROM ping_tree_campaigns
UNION ALL
SELECT 'leads', COUNT(*) FROM leads
UNION ALL
SELECT 'instance_users', COUNT(*) FROM instance_users
UNION ALL
SELECT 'instances', COUNT(*) FROM instances
ORDER BY table_name;

COMMIT;
