-- Pulsar decision logs table: Buyer qualification decision logs
CREATE TABLE IF NOT EXISTS pulsar_decision_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lead_id UUID,
    ping_id VARCHAR(255),
    buyer_id UUID NOT NULL,
    accepted BOOLEAN NOT NULL,
    final_bid_price DECIMAL(10, 2),
    rule_evaluations JSONB NOT NULL DEFAULT '[]',
    evaluated_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pulsar_decision_logs_lead_id ON pulsar_decision_logs(lead_id);
CREATE INDEX idx_pulsar_decision_logs_ping_id ON pulsar_decision_logs(ping_id);
CREATE INDEX idx_pulsar_decision_logs_buyer_id ON pulsar_decision_logs(buyer_id);
CREATE INDEX idx_pulsar_decision_logs_evaluated_at ON pulsar_decision_logs(evaluated_at DESC);




