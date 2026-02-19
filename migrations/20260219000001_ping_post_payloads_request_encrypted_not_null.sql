-- Make request_payload_encrypted NOT NULL on ping_payloads and post_payloads so all
-- INSERT paths (including validation-error and fallback) satisfy the constraint.
-- Backfill existing NULLs with empty string, then set default and NOT NULL.

-- ping_payloads
UPDATE ping_payloads SET request_payload_encrypted = '' WHERE request_payload_encrypted IS NULL;
ALTER TABLE ping_payloads ALTER COLUMN request_payload_encrypted SET DEFAULT '';
ALTER TABLE ping_payloads ALTER COLUMN request_payload_encrypted SET NOT NULL;

-- post_payloads (for consistency; post_payloads INSERTs already provide value or can use default)
UPDATE post_payloads SET request_payload_encrypted = '' WHERE request_payload_encrypted IS NULL;
ALTER TABLE post_payloads ALTER COLUMN request_payload_encrypted SET DEFAULT '';
ALTER TABLE post_payloads ALTER COLUMN request_payload_encrypted SET NOT NULL;
