-- Fix unique constraint on ping_trees to exclude soft-deleted rows
-- This allows creating a new ping tree with the same publisher_id and vertical
-- after soft-deleting a previous one

ALTER TABLE ping_trees DROP CONSTRAINT IF EXISTS unique_ping_tree_publisher_vertical;

CREATE UNIQUE INDEX unique_ping_tree_publisher_vertical_active 
  ON ping_trees(publisher_id, vertical) 
  WHERE deleted_at IS NULL;
