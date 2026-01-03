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

## Project Structure

- `crates/api` - Main API application
- `crates/core` - Shared core functionality
- `crates/utils` - Utility binaries
- `migrations/` - Database migration files

## CI/CD

- CI runs on pull requests and pushes to main/dev
- Deployments are automated via GitHub Actions
- Docker images are built in GHA and pushed to GHCR
- Fly.io deployments use pre-built images

