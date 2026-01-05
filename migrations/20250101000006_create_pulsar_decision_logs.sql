-- Create pulsar_decision_logs table
CREATE TABLE IF NOT EXISTS pulsar_decision_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lead_id UUID,
    ping_id VARCHAR(255),
    buyer_id UUID NOT NULL,
    accepted BOOLEAN NOT NULL,
    final_bid_price DECIMAL(10, 2),
    rule_evaluations JSONB,
    evaluated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pulsar_decision_logs_lead_id ON pulsar_decision_logs(lead_id);
CREATE INDEX idx_pulsar_decision_logs_buyer_id ON pulsar_decision_logs(buyer_id);
CREATE INDEX idx_pulsar_decision_logs_evaluated_at ON pulsar_decision_logs(evaluated_at);

