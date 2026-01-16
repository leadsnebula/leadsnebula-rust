-- Create ping_trees table
CREATE TABLE IF NOT EXISTS ping_trees (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL,
    publisher_id UUID NOT NULL,
    name VARCHAR(255) NOT NULL,
    vertical VARCHAR(50) NOT NULL,
    strategy VARCHAR(20) NOT NULL CHECK (strategy IN ('ping_post', 'fullpost')),
    status VARCHAR(20) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused')),
    priority INTEGER,
    deleted_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_ping_trees_instance FOREIGN KEY (instance_id) REFERENCES instances(id),
    CONSTRAINT fk_ping_trees_publisher FOREIGN KEY (publisher_id) REFERENCES publishers(id),
    CONSTRAINT unique_ping_tree_publisher_vertical UNIQUE (publisher_id, vertical),
    CONSTRAINT unique_ping_tree_name UNIQUE (publisher_id, vertical, name)
);

CREATE INDEX IF NOT EXISTS idx_ping_trees_deleted_at ON ping_trees(deleted_at);
CREATE INDEX IF NOT EXISTS idx_ping_trees_instance_id ON ping_trees(instance_id);
CREATE INDEX IF NOT EXISTS idx_ping_trees_publisher_id ON ping_trees(publisher_id);
CREATE INDEX IF NOT EXISTS idx_ping_trees_routing ON ping_trees(publisher_id, vertical, status) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_ping_trees_publisher_vertical ON ping_trees(publisher_id, vertical);

