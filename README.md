# LeadsNebula API (Rust)

High-performance lead routing and management API built with Rust, Axum, and Tokio.

## Architecture

- **Framework**: Axum (async web framework)
- **Runtime**: Tokio (async runtime)
- **Database**: PostgreSQL with SQLx
- **Cache**: Redis (Upstash)
- **Encryption**: Ring (AES-256-GCM)
- **Secrets**: AWS SSM Parameter Store
- **Monitoring**: Sentry

## Project Structure

```
crates/
├── api/          # Main application (Axum routes, server)
├── core/         # Core functionality (SSM, encryption, config)
├── models/       # Database models
├── services/     # Business logic services
└── utils/        # Utility functions
```

## Setup

1. **Install Rust** (if not already installed):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

2. **Set up AWS SSM Parameters** (recommended):
   ```bash
   # Configure AWS credentials
   aws configure
   # Or set environment variables:
   # export AWS_ACCESS_KEY_ID=your_key
   # export AWS_SECRET_ACCESS_KEY=your_secret
   # export AWS_REGION=us-east-1
   
   # Run the setup script to add database URLs and secrets to SSM
   ./scripts/setup-ssm-parameters.sh
   ```
   
   This will create encrypted parameters in AWS SSM:
   - `/leadsnebula/development/rust/db/connection_url`
   - `/leadsnebula/production/rust/db/connection_url`

3. **Local Development Setup** (without AWS SSM):
   ```bash
   # Create .env.local file (gitignored, not committed)
   cat > .env.local << 'EOF'
   ENVIRONMENT=development
   DATABASE_URL=postgresql://neondb_owner:password@ep-bitter-frog-ah9t1ome-pooler.c-3.us-east-1.aws.neon.tech/neondb?sslmode=require&channel_binding=require
   JWT_SECRET_KEY=$(openssl rand -hex 32)
   PORT=8080
   EOF
   
   # Or manually create .env.local and set:
   # - DATABASE_URL (required)
   # - JWT_SECRET_KEY (required, generate with: openssl rand -hex 32)
   # - ENVIRONMENT (optional, defaults to "development")
   # - PORT (optional, defaults to 8080)
   ```
   
   The app will automatically load `.env.local` if present, allowing local development without AWS credentials.

4. **Run migrations**:
   ```bash
   sqlx migrate run
   ```

5. **Run the server**:
   ```bash
   cargo run --bin leadsnebula-api
   ```

## Development

- **Run tests**: `cargo test`
- **Format code**: `cargo fmt`
- **Lint code**: `cargo clippy`
- **Build**: `cargo build --release`

## CI/CD and Deployment

This project uses GitHub Actions for CI/CD with automatic deployments to two environments:

- **Development** (`dev` branch) → `leadsnebula-rust-dev` on Fly.io
- **Production** (`main` branch) → `leadsnebula-rust` on Fly.io

### Quick Setup

1. **Initial Setup**: Follow [SETUP_CHECKLIST.md](./SETUP_CHECKLIST.md)
2. **Deployment Guide**: See [DEPLOYMENT.md](./DEPLOYMENT.md) for detailed information

### Automated Workflows

- **CI**: Runs on every push/PR (tests, linting, formatting)
- **Deploy Dev**: Auto-deploys on push to `dev` branch
- **Deploy Production**: Auto-deploys on push to `main` branch

All deployments automatically run database migrations before starting the application.

## Environment Variables

**Primary**: Configuration is stored in AWS SSM Parameter Store under `/leadsnebula/{env}/rust/...`.

**Fallback**: Environment variables (for local development without AWS):

- `ENVIRONMENT` - Environment name (development/production, default: "development")
- `PORT` - Server port (default: 8080)
- `DATABASE_URL` - PostgreSQL connection string (required if SSM unavailable)
- `REDIS_URL` - Redis connection string (optional)
- `SENTRY_DSN` - Sentry DSN (optional)
- `JWT_SECRET_KEY` - JWT signing secret (required if SSM unavailable)

### SSM Parameter Paths

The application looks for parameters in SSM using these paths:

- Database: `/leadsnebula/{env}/rust/db/connection_url`
- Redis: `/leadsnebula/{env}/rust/redis/connection_url`
- Sentry: `/leadsnebula/{env}/rust/sentry/dsn`
- JWT Secret: `/leadsnebula/{env}/rust/jwt/secret_key`

Where `{env}` is the value of `ENVIRONMENT` variable (development/production).

### Setting up SSM Parameters

Run the setup script to add all required parameters:

```bash
./scripts/setup-ssm-parameters.sh
```

See `scripts/README.md` for detailed instructions.

## Deployment

Deploy to Fly.io:
```bash
fly deploy
```

## License

MIT

