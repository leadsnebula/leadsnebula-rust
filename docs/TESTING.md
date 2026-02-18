# Testing

## Separation of concerns

- **`validate.sh`** – Pre-deployment / pre-commit validation only (no integration tests): fmt, clippy, unit tests, config checks, release build. Run before commit or before using deploy’s “skip_tests” hotfix path.
- **`autotestsall.sh`** – Test suite only: full integration + unit tests (Phase 1 + Phase 2), with optional Neon ephemeral DB. No fmt/clippy/audit/build; run after or alongside validate.sh for a full local gate.

## Canonical test suite (autotestsall.sh)

**`autotestsall.sh`** is the canonical full test suite. CI (Rust CI workflow) mirrors this behavior.

### Usage

- **Tests only:** `./autotestsall.sh` or `./autotestsall.sh --no-neon` (if `DATABASE_URL` is already set).
- **Full pre-deployment locally:** run `./validate.sh` then `./autotestsall.sh` (or `./autotestsall.sh --no-neon` if you already have a DB).

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

- **Local:** Pre-validation: `./validate.sh`. Tests: `./autotestsall.sh` (optionally `--no-neon`). Full gate: validate.sh then autotestsall.sh.
- **CI:** Lint (fmt + clippy), unit tests, integration (one ephemeral Neon branch; Phase 1 then Phase 2), coverage, then build release on push to dev. Audit runs in a separate job.
