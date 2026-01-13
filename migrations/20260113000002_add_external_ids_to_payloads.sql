-- Add textual external ping/post id columns for easier lookup and linking
BEGIN;

ALTER TABLE ping_payloads ADD COLUMN IF NOT EXISTS external_ping_id TEXT;
ALTER TABLE post_payloads ADD COLUMN IF NOT EXISTS external_post_id TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS ux_ping_payloads_external_ping ON ping_payloads (external_ping_id) WHERE external_ping_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_post_payloads_external_post ON post_payloads (external_post_id) WHERE external_post_id IS NOT NULL;

COMMIT;
