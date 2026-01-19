-- Create ping_trees table
-- Note: publisher_id was removed in migration 20260120000004_remove_publisher_id_from_ping_trees.sql
-- Publishers are now linked via ping_tree_publishers join table (created in 20260120000002)
CREATE TABLE IF NOT EXISTS ping_trees (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL,
    name VARCHAR(255) NOT NULL,
    vertical VARCHAR(50) NOT NULL,
    strategy VARCHAR(20) NOT NULL CHECK (strategy IN ('ping_post', 'fullpost')),
    status VARCHAR(20) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused')),
    priority INTEGER,
    deleted_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_ping_trees_instance FOREIGN KEY (instance_id) REFERENCES instances(id),
    CONSTRAINT unique_ping_tree_name UNIQUE (instance_id, vertical, name)
);

CREATE INDEX IF NOT EXISTS idx_ping_trees_deleted_at ON ping_trees(deleted_at);
CREATE INDEX IF NOT EXISTS idx_ping_trees_instance_id ON ping_trees(instance_id);
CREATE INDEX IF NOT EXISTS idx_ping_trees_routing ON ping_trees(vertical, status) WHERE deleted_at IS NULL;

