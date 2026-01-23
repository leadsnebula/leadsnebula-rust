-- Analyze the prechecks query performance
-- This query is used to find campaign_id and buyer_id before inserting a lead

-- Check indexes on campaigns table
SELECT 
    tablename,
    indexname,
    indexdef
FROM pg_indexes
WHERE tablename IN ('campaigns', 'buyers', 'verticals', 'ping_trees', 'ping_tree_publishers')
ORDER BY tablename, indexname;

-- Analyze the query execution plan
EXPLAIN ANALYZE
SELECT 
    c.id AS campaign_id,
    COALESCE(c.buyer_id, b_vertical.id) AS effective_buyer_id,
    EXISTS(
        SELECT 1 FROM ping_trees pt
        INNER JOIN ping_tree_publishers ptp ON pt.id = ptp.ping_tree_id
        WHERE ptp.publisher_id = '00000000-0000-0000-0000-000000000000'::uuid 
        AND pt.vertical = 'solar' 
        AND pt.deleted_at IS NULL
    ) AS has_ping_tree
FROM (VALUES (true)) AS dummy
LEFT JOIN campaigns c ON (
    (c.campaign_token = '' AND '' != '') OR 
    (c.vertical = 'solar' AND c.buyer_id IN (
        SELECT b.id FROM buyers b 
        WHERE b.vertical_id = (
            SELECT v.id FROM verticals v 
            WHERE v.slug = 'solar' AND v.is_active = true
        ) AND b.deleted_at IS NULL
    ))
) AND c.deleted_at IS NULL
LEFT JOIN buyers b_vertical ON 
    b_vertical.vertical_id = (
        SELECT v2.id FROM verticals v2 
        WHERE v2.slug = 'solar' AND v2.is_active = true
    ) 
    AND c.id IS NULL 
    AND b_vertical.deleted_at IS NULL
LIMIT 1;

-- Check for missing indexes
-- campaigns table should have indexes on:
-- - campaign_token (if used frequently)
-- - vertical + deleted_at
-- - buyer_id + vertical + deleted_at

-- buyers table should have indexes on:
-- - vertical_id + deleted_at

-- verticals table should have indexes on:
-- - slug + is_active (unique constraint should cover this)

-- ping_trees table should have indexes on:
-- - vertical + deleted_at

-- ping_tree_publishers should have indexes on:
-- - publisher_id + ping_tree_id (composite index for the EXISTS subquery)
