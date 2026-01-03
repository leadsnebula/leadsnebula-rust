-- Ping trees table: Routing trees for ping/post strategy
CREATE TABLE IF NOT EXISTS ping_trees (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL,
    publisher_id UUID NOT NULL,
    name VARCHAR(255) NOT NULL,
    vertical VARCHAR(100) NOT NULL,
    strategy VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    priority INTEGER,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_ping_trees_status CHECK (status IN ('active', 'paused')),
    CONSTRAINT check_ping_trees_strategy CHECK (strategy IN ('ping_post', 'fullpost')),
    CONSTRAINT idx_ping_trees_unique_name UNIQUE (publisher_id, vertical, name),
    CONSTRAINT idx_ping_trees_unique_publisher_vertical UNIQUE (publisher_id, vertical)
);

CREATE INDEX idx_ping_trees_instance_id ON ping_trees(instance_id);
CREATE INDEX idx_ping_trees_publisher_id ON ping_trees(publisher_id);
CREATE INDEX idx_ping_trees_deleted_at ON ping_trees(deleted_at);
CREATE INDEX idx_ping_trees_routing ON ping_trees(publisher_id, vertical, status);




