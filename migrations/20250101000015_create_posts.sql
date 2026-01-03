-- Posts table: Post requests to buyers
CREATE TABLE IF NOT EXISTS posts (
    id BIGSERIAL PRIMARY KEY,
    post_id TEXT NOT NULL UNIQUE,
    promise_id TEXT,
    result TEXT,
    buyer_response JSONB,
    error_message TEXT,
    response_time_ms INTEGER,
    sent_at TIMESTAMPTZ,
    lead_id UUID,
    request_type VARCHAR(20) NOT NULL DEFAULT 'post',
    buyer_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_posts_request_type CHECK (request_type IN ('post', 'fullpost'))
);

CREATE INDEX idx_posts_post_id ON posts(post_id);
CREATE INDEX idx_posts_promise_id ON posts(promise_id);
CREATE INDEX idx_posts_lead_id ON posts(lead_id);
CREATE INDEX idx_posts_buyer_id ON posts(buyer_id);
CREATE INDEX idx_posts_result ON posts(result);
CREATE INDEX idx_posts_request_type ON posts(request_type);
CREATE INDEX idx_posts_created_at ON posts(created_at DESC);
CREATE INDEX idx_posts_buyer_result_created ON posts(buyer_id, result, created_at DESC);
CREATE INDEX idx_posts_accepted_only ON posts(result, created_at DESC) WHERE result = 'accepted';
CREATE INDEX idx_posts_buyer_response_gin ON posts USING GIN (buyer_response);




