-- Remove publisher_id from ping_trees table
-- Ping trees are now owned by instance, not individual publishers
-- Publishers are assigned via ping_tree_publishers join table

-- Drop old constraints that reference publisher_id
ALTER TABLE ping_trees DROP CONSTRAINT IF EXISTS unique_ping_tree_publisher_vertical;
ALTER TABLE ping_trees DROP CONSTRAINT IF EXISTS unique_ping_tree_name;
ALTER TABLE ping_trees DROP CONSTRAINT IF EXISTS fk_ping_trees_publisher;

-- Drop publisher_id column
ALTER TABLE ping_trees DROP COLUMN IF EXISTS publisher_id;

-- Update name uniqueness to (instance_id, vertical, name)
ALTER TABLE ping_trees ADD CONSTRAINT unique_ping_tree_name 
  UNIQUE (instance_id, vertical, name);

-- Drop old index that included publisher_id
DROP INDEX IF EXISTS idx_ping_trees_publisher_id;
DROP INDEX IF EXISTS idx_ping_trees_publisher_vertical;
DROP INDEX IF EXISTS idx_ping_trees_routing;

-- Recreate routing index without publisher_id (for future use if needed)
CREATE INDEX IF NOT EXISTS idx_ping_trees_routing 
  ON ping_trees(vertical, status) 
  WHERE deleted_at IS NULL;
