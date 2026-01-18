-- Cleanup script to remove all test verticals except solar
-- WARNING: This will DELETE all verticals except the one with slug='solar'
-- Run this ONLY on your main/dev database, NOT on production
--
-- Usage:
--   psql $DATABASE_URL -f scripts/cleanup-test-verticals.sql

BEGIN;

-- Delete test verticals (keep only solar)
-- First, delete publisher_verticals that reference non-solar verticals
DELETE FROM publisher_verticals
WHERE vertical_id NOT IN (SELECT id FROM verticals WHERE slug = 'solar');

-- Delete buyer_integrations that reference non-solar verticals
DELETE FROM buyer_integrations
WHERE vertical_id NOT IN (SELECT id FROM verticals WHERE slug = 'solar');

-- Delete buyer_qualification_configs that reference non-solar verticals
DELETE FROM buyer_qualification_configs
WHERE vertical_id NOT IN (SELECT id FROM verticals WHERE slug = 'solar');

-- Delete leads that reference non-solar verticals (test data)
DELETE FROM leads
WHERE vertical_id NOT IN (SELECT id FROM verticals WHERE slug = 'solar');

-- Delete campaigns that reference non-solar verticals (via buyer_id or publisher_id)
-- Note: campaigns don't directly reference verticals, but we can identify test campaigns
-- by their association with test publishers/buyers
DELETE FROM campaigns
WHERE id IN (
    SELECT c.id FROM campaigns c
    LEFT JOIN publishers p ON c.publisher_id = p.id
    LEFT JOIN buyers b ON c.buyer_id = b.id
    WHERE (p.id IS NULL OR p.deleted_at IS NOT NULL)
       OR (b.id IS NULL)
       OR c.name LIKE 'Test%'
       OR c.name LIKE 'test%'
);

-- Now delete non-solar verticals
DELETE FROM verticals
WHERE slug != 'solar';

-- Show summary
SELECT 
    'verticals' as table_name, 
    COUNT(*) as remaining_count,
    string_agg(slug, ', ') as slugs
FROM verticals
UNION ALL
SELECT 'publisher_verticals', COUNT(*), '' FROM publisher_verticals
UNION ALL
SELECT 'buyer_integrations', COUNT(*), '' FROM buyer_integrations
UNION ALL
SELECT 'buyer_qualification_configs', COUNT(*), '' FROM buyer_qualification_configs
ORDER BY table_name;

COMMIT;
