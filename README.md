# LeadsNebula Rust API

Rust API for the LeadsNebula platform.

## Documentation

**Note**: This repository contains only application code. All documentation, setup guides, and operational procedures are maintained externally (wiki, knowledge base, etc.). The README.md file is kept minimal and focused on essential development information only.

## Development

### Prerequisites

- Rust 1.75+
- PostgreSQL
- AWS credentials configured for SSM access
- Redis (optional, for caching and rate limiting)

### Local Setup

1. Copy environment variables:
```bash
cp .env.local.example .env.local
```

2. Set required environment variables:
- `DATABASE_URL` - PostgreSQL connection string
- `ENVIRONMENT` - Environment name (development, production)
- `JWT_SECRET` - Secret key for JWT tokens
- `AWS_ACCESS_KEY_ID` - AWS access key
- `AWS_SECRET_ACCESS_KEY` - AWS secret key
- `REDIS_URL` - Redis connection string (optional)

3. Run migrations:
```bash
cargo run --bin run-migrations
```

4. Start the server:
```bash
cargo run --bin leadsnebula-api
```

### Testing with Neon (ephemeral branches) ⚠️

To run DB-backed tests in an isolated, ephemeral Postgres environment that mirrors CI, use Neon ephemeral branches. See `docs/TESTING_WITH_NEON.md` for details and a safe helper script.

Example:

```bash
# create ephemeral branch, run workspace tests, then cleanup
TEST_EPHEMERAL_NEON=true NEON_API_KEY=... NEON_PROJECT_ID=... ./scripts/test-neon-ephemeral.sh -- cargo test --workspace
```

Be cautious: setting `TEST_USE_DEV=true` requires `TEST_RUN_ID` to avoid accidental tests against shared dev resources.


### Pre-commit Validation

Before committing code, run the validation script to ensure CI will pass:

```bash
./validate.sh          # Full validation (all tests) - USE BEFORE COMMITTING
./validate.sh --fast    # Fast validation (unit tests only) - DEVELOPMENT ONLY
```

**Purpose**: The `validate.sh` script performs comprehensive pre-commit checks including:
- Code formatting (`cargo fmt`)
- Linting (`cargo clippy`)
- Unit tests (with optional fast mode)
- Integration tests (skipped in fast mode)
- Build verification
- Security audits (`cargo audit`, `cargo deny`)
- Workflow file validation

**⚠️ Important**: Fast mode (`--fast`) skips database integration tests and should only be used during rapid development iteration. Always run the full validation (`./validate.sh`) before committing to ensure all tests pass.

## Project Structure

- `crates/api` - Main API application
- `crates/core` - Shared core functionality
- `crates/utils` - Utility binaries
- `migrations/` - Database migration files
- `scripts/` - Utility scripts for database operations and infrastructure management (see `scripts/README.md`)
- `validate.sh` - Pre-commit validation script (see [Pre-commit Validation](#pre-commit-validation))

## CI/CD

- CI runs on pull requests and pushes to main/dev
- Deployments are automated via GitHub Actions
- Docker images are built in GHA and pushed to GHCR
- Fly.io deployments use pre-built images

## Production Configuration

### Logging

For production deployments, set `RUST_LOG=warn` in the Fly.io environment to disable feature-gated tracing and eliminate logging overhead. This ensures zero tracing overhead in production builds.

**Fly.io Configuration:**
```bash
fly secrets set RUST_LOG=warn
```

**Note**: Keep `RUST_LOG=debug` or `RUST_LOG=info` for development/staging environments to enable detailed logging and debugging.