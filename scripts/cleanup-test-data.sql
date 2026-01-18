-- Cleanup script to remove all test data from main database
-- WARNING: This will DELETE all test records from publishers, buyers, campaigns, and ping_trees
-- Run this ONLY on your main/dev database, NOT on production
--
-- Usage:
--   psql $DATABASE_URL -f scripts/cleanup-test-data.sql
--   Or: cat scripts/cleanup-test-data.sql | psql $DATABASE_URL

BEGIN;

-- Delete in order to respect foreign key constraints (children first, then parents)

-- 1. Delete test leads first (references publishers, campaigns, buyers)
DELETE FROM leads
WHERE 
    publisher_id IN (SELECT id FROM publishers WHERE name LIKE 'Test%' OR name LIKE 'test%' OR email LIKE '%@test.%' OR email LIKE '%test%@%' OR email LIKE '%@example.com' OR api_key_prefix LIKE 'pk_test%' OR api_key_hash LIKE 'hash_%')
    OR event_id LIKE 'evt_%'
    OR session_id LIKE 'sess_%'
    OR post_id LIKE 'post_%'
    OR post_id LIKE 'INPROG_%'
    OR promise_id LIKE 'PROMISE_%';

-- 2. Delete ping_tree_campaigns (references ping_trees and campaigns)
DELETE FROM ping_tree_campaigns
WHERE 
    ping_tree_id IN (SELECT id FROM ping_trees WHERE name LIKE 'Test%' OR name LIKE 'test%' OR publisher_id IN (SELECT id FROM publishers WHERE name LIKE 'Test%' OR name LIKE 'test%' OR email LIKE '%@test.%' OR email LIKE '%test%@%' OR email LIKE '%@example.com'))
    OR campaign_id IN (SELECT id FROM campaigns WHERE name LIKE 'Test%' OR name LIKE 'test%');

-- 3. Delete ping trees (references publishers)
DELETE FROM ping_trees
WHERE 
    name LIKE 'Test%'
    OR name LIKE 'test%'
    OR publisher_id IN (SELECT id FROM publishers WHERE name LIKE 'Test%' OR name LIKE 'test%' OR email LIKE '%@test.%' OR email LIKE '%test%@%' OR email LIKE '%@example.com' OR api_key_prefix LIKE 'pk_test%' OR api_key_hash LIKE 'hash_%');

-- 4. Delete campaigns (references publishers and buyers)
DELETE FROM campaigns
WHERE 
    name LIKE 'Test%'
    OR name LIKE 'test%'
    OR publisher_id IN (SELECT id FROM publishers WHERE name LIKE 'Test%' OR name LIKE 'test%' OR email LIKE '%@test.%' OR email LIKE '%test%@%' OR email LIKE '%@example.com' OR api_key_prefix LIKE 'pk_test%' OR api_key_hash LIKE 'hash_%')
    OR buyer_id IN (SELECT id FROM buyers WHERE name LIKE 'Test%' OR name LIKE 'test%');

-- 5. Delete buyers (no dependencies)
DELETE FROM buyers
WHERE 
    name LIKE 'Test%'
    OR name LIKE 'test%';

-- 6. Delete publishers (no dependencies after leads/campaigns/ping_trees are gone)
DELETE FROM publishers
WHERE 
    name LIKE 'Test%' 
    OR name LIKE 'test%'
    OR email LIKE '%@test.%'
    OR email LIKE '%test%@%'
    OR email LIKE '%@example.com'
    OR api_key_prefix LIKE 'pk_test%'
    OR api_key_hash LIKE 'hash_%';

-- 7. Delete test instance_users (references instances)
DELETE FROM instance_users
WHERE 
    email LIKE '%@test.%'
    OR email LIKE '%test%@%'
    OR email LIKE '%@example.com';

-- 8. Delete orphaned instances (after instance_users are deleted)
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
