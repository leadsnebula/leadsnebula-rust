-- Fix unique constraint on ping_trees to exclude soft-deleted rows
-- Note: This migration is now a no-op since publisher_id was removed from ping_trees
-- The unique constraint on (publisher_id, vertical) is now handled by ping_tree_publishers table
-- This migration is kept for historical reference but does nothing

-- Drop old constraint if it exists (from before publisher_id removal)
ALTER TABLE ping_trees DROP CONSTRAINT IF EXISTS unique_ping_tree_publisher_vertical;
