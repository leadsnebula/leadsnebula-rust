-- Migrate existing publisher_id data to ping_tree_publishers join table
-- Set default revshare_percentage = 80.0 (generous starting point for negotiation)

INSERT INTO ping_tree_publishers (ping_tree_id, publisher_id, vertical, revshare_percentage, created_at, updated_at)
SELECT id, publisher_id, vertical, 80.0, created_at, updated_at
FROM ping_trees
WHERE deleted_at IS NULL AND publisher_id IS NOT NULL
ON CONFLICT (ping_tree_id, publisher_id) DO NOTHING;
