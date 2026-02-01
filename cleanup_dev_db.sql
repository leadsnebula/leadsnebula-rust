-- Cleanup script for dev database
-- Keeps only specified test records and their configurations
-- Run this script against the dev database

BEGIN;

-- UUIDs to keep (hardcoded for PostgreSQL)
DO $$
DECLARE
    keep_publisher_id UUID := '0d2a06f2-af57-40c8-b3c7-b61fc7621de6';
    keep_buyer_1_id UUID := '06dd332e-cf8c-42ea-8c6a-4594d2244db3';
    keep_buyer_2_id UUID := '942fb984-8db6-4804-bcda-90fa478a20a1';
    keep_campaign_1_id UUID := 'd5d13400-efb2-4c05-b355-5c32f1bd950e';
    keep_campaign_2_id UUID := '668a0b94-f2ad-4f83-887f-3c3f0a2ea809';
    keep_ping_tree_1_id UUID := '3fcb8660-443f-44c3-b0eb-f262cc09fb2b';
    keep_ping_tree_2_id UUID := '7ef1c2f7-d3df-48ca-af98-542bfa5eedc2';
BEGIN
    -- Step 1: Delete all leads not associated with kept records
    -- This will cascade delete buyer_responses and payloads
    -- Keep leads that are associated with kept campaigns, buyers, or publishers
    DELETE FROM leads
    WHERE NOT (
        (campaign_id IN (keep_campaign_1_id, keep_campaign_2_id))
        OR (buyer_id IN (keep_buyer_1_id, keep_buyer_2_id))
        OR (publisher_id = keep_publisher_id)
    );

    -- Step 2: Delete ping_tree_campaigns not linked to kept ping trees/campaigns
    DELETE FROM ping_tree_campaigns
    WHERE ping_tree_id NOT IN (keep_ping_tree_1_id, keep_ping_tree_2_id)
       OR campaign_id NOT IN (keep_campaign_1_id, keep_campaign_2_id);

    -- Step 3: Delete ping_tree_publishers not linked to kept ping trees/publishers
    DELETE FROM ping_tree_publishers
    WHERE ping_tree_id NOT IN (keep_ping_tree_1_id, keep_ping_tree_2_id)
       OR publisher_id != keep_publisher_id;

    -- Step 4: Delete campaigns not in the keep list
    -- This will cascade delete ping_tree_campaigns (but we already cleaned those)
    DELETE FROM campaigns
    WHERE id NOT IN (keep_campaign_1_id, keep_campaign_2_id);

    -- Step 5: Delete buyers not in the keep list
    -- This will cascade delete buyer_zip_lists, buyer_qualification_configs
    DELETE FROM buyers
    WHERE id NOT IN (keep_buyer_1_id, keep_buyer_2_id);

    -- Step 6: Delete publishers not in the keep list
    -- This will cascade delete publisher_verticals, ping_tree_publishers (but we already cleaned those)
    DELETE FROM publishers
    WHERE id != keep_publisher_id;

    -- Step 7: Delete ping trees not in the keep list
    -- This will cascade delete ping_tree_campaigns and ping_tree_publishers (but we already cleaned those)
    DELETE FROM ping_trees
    WHERE id NOT IN (keep_ping_tree_1_id, keep_ping_tree_2_id);

    -- Step 8: Clean up any orphaned records in related tables
    -- Buyer zip codes (should be cascade deleted, but clean up any orphans)
    DELETE FROM buyer_zip_codes
    WHERE buyer_zip_list_id NOT IN (
        SELECT id FROM buyer_zip_lists WHERE buyer_id IN (keep_buyer_1_id, keep_buyer_2_id)
    );

    DELETE FROM buyer_zip_lists
    WHERE buyer_id NOT IN (keep_buyer_1_id, keep_buyer_2_id);

    -- Buyer qualification configs (should be cascade deleted, but clean up any orphans)
    DELETE FROM buyer_qualification_configs
    WHERE buyer_id NOT IN (keep_buyer_1_id, keep_buyer_2_id);

    -- Publisher verticals (should be cascade deleted, but clean up any orphans)
    DELETE FROM publisher_verticals
    WHERE publisher_id != keep_publisher_id;

    RAISE NOTICE 'Cleanup completed successfully';
END $$;

COMMIT;

-- Verify cleanup
SELECT 'Publishers remaining:' as check_type, COUNT(*) as count FROM publishers;
SELECT 'Buyers remaining:' as check_type, COUNT(*) as count FROM buyers;
SELECT 'Campaigns remaining:' as check_type, COUNT(*) as count FROM campaigns;
SELECT 'Ping trees remaining:' as check_type, COUNT(*) as count FROM ping_trees;
SELECT 'Ping tree campaigns remaining:' as check_type, COUNT(*) as count FROM ping_tree_campaigns;
SELECT 'Ping tree publishers remaining:' as check_type, COUNT(*) as count FROM ping_tree_publishers;
SELECT 'Leads remaining:' as check_type, COUNT(*) as count FROM leads;
