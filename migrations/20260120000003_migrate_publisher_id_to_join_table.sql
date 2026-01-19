-- Migrate existing publisher_id data to ping_tree_publishers join table
-- Set default revshare_percentage = 80.0 (generous starting point for negotiation)
-- This migration is conditional: only runs if publisher_id column exists in ping_trees
-- (for databases that were created before publisher_id was removed)

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'ping_trees' AND column_name = 'publisher_id'
    ) THEN
        INSERT INTO ping_tree_publishers (ping_tree_id, publisher_id, vertical, revshare_percentage, created_at, updated_at)
        SELECT id, publisher_id, vertical, 80.0, created_at, updated_at
        FROM ping_trees
        WHERE deleted_at IS NULL AND publisher_id IS NOT NULL
        ON CONFLICT (ping_tree_id, publisher_id) DO NOTHING;
    END IF;
END $$;
