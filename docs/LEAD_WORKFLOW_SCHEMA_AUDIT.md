# Lead workflow – schema and INSERT checklist

Use this when adding or changing INSERTs/UPDATEs in the lead flow so deployments don’t fail on DB constraints.

## Tables and NOT NULL (current after migrations)

### leads
- **NOT NULL:** uuid, event_id, vertical_id, request_type, strategy, status, tcpa_consent, tcpa_language, is_test, vertical_data, created_at, updated_at, submitted_at, publisher_id, post_id (20260112000004).
- **Nullable:** buyer_id, campaign_id (20260217000001), promise_id, lead_id, session_id, PII columns, etc.
- **INSERT must provide:** updated_at (no default in some paths), submitted_at, post_id (use `''` for error leads), publisher_id.

### pings
- Columns used in INSERT: ping_id, lead_id, promise_id, state, sent_at, created_at.
- No `updated_at` in current pings schema (id, ping_id, lead_id, promise_id, state, sent_at, created_at per usage).

### ping_payloads
- **NOT NULL (after 20260219000001):** payload, created_at, updated_at, request_payload_encrypted.
- **Nullable:** ping_id (VARCHAR), lead_id, response_payload_encrypted, external_ping_id.
- **INSERT must provide:** request_payload_encrypted (use `''` for validation-error or fallback), created_at, updated_at (or rely on DEFAULT now()).

### post_payloads
- **NOT NULL:** payload, created_at, updated_at; request_payload_encrypted (after 20260219000001).
- **INSERT must provide:** created_at, updated_at (or DEFAULT), request_payload_encrypted when writing (or DEFAULT '').

### buyer_responses
- **NOT NULL:** payload, created_at.
- **Nullable:** lead_id, ping_id, post_id, buyer_id, campaign_id, response_payload_encrypted.

## Code paths that INSERT (all must match schema)

| Location | Table | Fix applied |
|--------|------|-------------|
| `leads.rs` `persist_failed_lead` | leads | updated_at added earlier |
| `leads.rs` `persist_failed_lead` | pings | OK |
| `leads.rs` `persist_failed_lead` | ping_payloads | request_payload_encrypted, updated_at added |
| `write_behind_queue.rs` batch_create_leads | leads | updated_at added |
| `write_behind_queue.rs` batch_create_leads | pings | OK |
| `write_behind_queue.rs` batch_create_leads | ping_payloads | request_payload_encrypted, updated_at added |
| `write_behind_queue.rs` PayloadUpdate | ping_payloads (fallback INSERT) | request_payload_encrypted = '' added |
| `write_behind_queue.rs` PayloadUpdate | post_payloads | updated_at added; request/response already provided or optional |
| `ping_tree_router.rs` | buyer_responses | OK (payload, created_at) |

## Before adding new INSERTs

1. List every column of the table (from migrations, in order).
2. Mark NOT NULL columns and those with no DEFAULT.
3. Ensure the INSERT column list and VALUES count match and every required column is bound or has DEFAULT.
4. For leads, always include updated_at and submitted_at.
5. For ping_payloads/post_payloads, always include request_payload_encrypted (use `''` if no request body).
