-- Data processing agreements table: Tracks 3rd party DPAs
CREATE TABLE IF NOT EXISTS data_processing_agreements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    third_party_name VARCHAR(255) NOT NULL,
    third_party_contact VARCHAR(255),
    agreement_date DATE NOT NULL,
    expiry_date DATE,
    agreement_terms TEXT,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_data_processing_agreements_third_party_name ON data_processing_agreements(third_party_name);
CREATE INDEX idx_data_processing_agreements_active ON data_processing_agreements(active);
CREATE INDEX idx_data_processing_agreements_expiry_date ON data_processing_agreements(expiry_date);
CREATE INDEX idx_data_processing_agreements_name_active ON data_processing_agreements(third_party_name, active);




