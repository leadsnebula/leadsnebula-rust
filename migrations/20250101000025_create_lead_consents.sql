-- Lead consents table: Tracks user consent for marketing/3rd party sharing
CREATE TABLE IF NOT EXISTS lead_consents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lead_id UUID NOT NULL,
    consent_type VARCHAR(100) NOT NULL,
    consented BOOLEAN NOT NULL DEFAULT false,
    consented_at TIMESTAMPTZ,
    consent_method VARCHAR(100),
    consent_text TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT idx_lead_consents_unique UNIQUE (lead_id, consent_type)
);

CREATE INDEX idx_lead_consents_lead_id ON lead_consents(lead_id);
CREATE INDEX idx_lead_consents_consent_type ON lead_consents(consent_type);
CREATE INDEX idx_lead_consents_consented ON lead_consents(consented);
CREATE INDEX idx_lead_consents_consented_at ON lead_consents(consented_at);




