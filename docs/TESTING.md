# Testing

## Canonical test suite

**`autotestsall.sh`** is the canonical full test suite. CI (Rust CI workflow) mirrors this behavior.

### Phases

1. **Phase 1 – DB-heavy integration (sequential)**  
   Run with `--test-threads=1` to avoid migration table races. Tests:
   - `integration_auth`
   - `integration_publisher_crud`
   - `integration_leads_endpoint`
   - `integration_carina_e2e`
   - `integration_email`

2. **Phase 2 – Unit + lib + other tests (parallel)**  
   All other targets (unit tests, lib tests, remaining integration tests) run in parallel (e.g. 8 threads in CI).

### Heavy tests

Tests that take 30–40s each are **skipped** unless `RUN_HEAVY_TESTS=true` is set (e.g. in `.env.local` for local runs). CI does not set this.

### Local vs CI

- **Local:** `./autotestsall.sh` (optionally `--no-neon` if `DATABASE_URL` is already set).
- **CI:** One ephemeral Neon branch per run; Phase 1 then Phase 2; coverage is generated in the same job after tests.
