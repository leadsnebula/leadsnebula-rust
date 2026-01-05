-- Create buyer_zip_lists table
CREATE TABLE IF NOT EXISTS buyer_zip_lists (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_id UUID NOT NULL,
    name VARCHAR(255) NOT NULL,
    list_type VARCHAR(20) NOT NULL, -- 'blacklist' or 'whitelist'
    price_adjustment DECIMAL(10, 2) DEFAULT 0.0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_buyer_zip_lists_buyer FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE CASCADE,
    CONSTRAINT check_list_type CHECK (list_type IN ('blacklist', 'whitelist'))
);

-- Create buyer_zip_codes table
CREATE TABLE IF NOT EXISTS buyer_zip_codes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_zip_list_id UUID NOT NULL,
    zip VARCHAR(5) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_buyer_zip_codes_list FOREIGN KEY (buyer_zip_list_id) REFERENCES buyer_zip_lists(id) ON DELETE CASCADE,
    CONSTRAINT unique_list_zip UNIQUE (buyer_zip_list_id, zip)
);

-- Create indexes
-- Composite index for efficient buyer + list_type queries (most common lookup pattern)
CREATE INDEX IF NOT EXISTS idx_buyer_zip_lists_buyer_type ON buyer_zip_lists(buyer_id, list_type);
-- Index for buyer_id lookups
CREATE INDEX IF NOT EXISTS idx_buyer_zip_lists_buyer_id ON buyer_zip_lists(buyer_id);
-- Index for list_type lookups
CREATE INDEX IF NOT EXISTS idx_buyer_zip_lists_list_type ON buyer_zip_lists(list_type);
-- Index for list_id lookups (already covered by FK, but explicit for clarity)
CREATE INDEX IF NOT EXISTS idx_buyer_zip_codes_list_id ON buyer_zip_codes(buyer_zip_list_id);
-- Index for ZIP code lookups (for queries like "which lists contain this ZIP?")
CREATE INDEX IF NOT EXISTS idx_buyer_zip_codes_zip ON buyer_zip_codes(zip);
-- Composite index for efficient list_id + zip lookups (the unique constraint already creates this, but explicit for clarity)
-- Note: The unique constraint on (buyer_zip_list_id, zip) already creates an index, but we document it here
