# Deployment Guide

This document describes the CI/CD setup and deployment process for the LeadsNebula Rust API.

## Environments

We have two environments:

1. **Development** (`dev` branch)
   - App: `leadsnebula-rust-dev`
   - Database: Development database branch
   - Auto-deploys on push to `dev` branch
   - Can scale to zero when not in use

2. **Production** (`main` branch)
   - App: `leadsnebula-rust`
   - Database: Production database
   - Auto-deploys on push to `main` branch
   - Manual deployment also available via GitHub Actions
   - Requires approval if protection rules are enabled

## Prerequisites

### 1. Fly.io Setup

You need separate Fly.io apps for each environment:

```bash
# Create dev app
flyctl apps create leadsnebula-rust-dev

# Production app should already exist
# flyctl apps create leadsnebula-rust
```

### 2. GitHub Secrets

Add the following secrets to your GitHub repository:

- `FLY_API_TOKEN`: Your Fly.io API token (get it with `flyctl auth token`)

Optional:
- `SLACK_WEBHOOK_URL`: For production deployment notifications

### 3. AWS SSM Parameters

Each environment needs its own SSM parameters. Set them up using the script:

```bash
# For development
ENVIRONMENT=development ./scripts/setup-ssm-parameters.sh

# For production
ENVIRONMENT=production ./scripts/setup-ssm-parameters.sh
```

Required SSM parameters for each environment:
- `/leadsnebula/{env}/rust/db/connection_url` - Database connection string
- `/leadsnebula/{env}/rust/jwt/secret_key` - JWT signing secret
- `/leadsnebula/{env}/rust/sentry/dsn` - Sentry DSN (optional)
- `/leadsnebula/{env}/rust/redis/connection_url` - Redis connection (optional)

### 4. Fly.io Secrets

Set environment-specific secrets in Fly.io:

```bash
# Development
flyctl secrets set -a leadsnebula-rust-dev \
  ENVIRONMENT=development \
  AWS_REGION=us-east-1

# Production
flyctl secrets set -a leadsnebula-rust \
  ENVIRONMENT=production \
  AWS_REGION=us-east-1
```

### 5. AWS IAM Permissions

Each Fly.io app needs AWS credentials to access SSM. Set up IAM roles and attach them:

```bash
# Create IAM user for each environment (or use one with proper path restrictions)
# Then set the credentials as Fly.io secrets:

flyctl secrets set -a leadsnebula-rust-dev \
  AWS_ACCESS_KEY_ID=your-key \
  AWS_SECRET_ACCESS_KEY=your-secret

flyctl secrets set -a leadsnebula-rust \
  AWS_ACCESS_KEY_ID=your-key \
  AWS_SECRET_ACCESS_KEY=your-secret
```

## Deployment Process

### Automatic Deployment

1. **Development**: Push to `dev` branch → automatically deploys
2. **Production**: Push to `main` branch → automatically deploys

### Manual Deployment

You can trigger deployments manually via GitHub Actions:

1. Go to Actions tab in GitHub
2. Select the workflow (Deploy to Dev or Deploy to Production)
3. Click "Run workflow"
4. Select the branch and click "Run workflow"

### Local Deployment (for testing)

```bash
# Deploy to dev
flyctl deploy --config fly.dev.toml -a leadsnebula-rust-dev

# Deploy to production
flyctl deploy --config fly.toml -a leadsnebula-rust
```

## Database Migrations

Migrations run automatically via the `release_command` in each `fly.toml` file. The migration runner:

1. Connects to the database using SSM parameters
2. Runs all pending migrations
3. Exits successfully (app continues to start even if migrations fail - check logs)

To run migrations manually:

```bash
# SSH into the app
flyctl ssh console -a leadsnebula-rust-dev

# Run migrations
/app/run-migrations
```

## Monitoring

### Health Checks

Each environment has health check endpoints:
- Development: https://leadsnebula-rust-dev.fly.dev/health
- Production: https://leadsnebula-rust.fly.dev/health

### Logs

View logs for each environment:

```bash
# Development
flyctl logs -a leadsnebula-rust-dev

# Production
flyctl logs -a leadsnebula-rust
```

### Sentry

Errors are automatically sent to Sentry. Check your Sentry dashboard for:
- Development environment errors
- Production environment errors

## Rollback

To rollback a deployment:

```bash
# List releases
flyctl releases -a leadsnebula-rust

# Rollback to a specific release
flyctl releases rollback <release-id> -a leadsnebula-rust
```

## Branch Strategy

- `dev` → Development environment (auto-deploy)
- `main` → Production environment (auto-deploy)

### Typical Workflow

1. Create feature branch from `dev`
2. Make changes and test locally
3. Merge to `dev` → auto-deploys to development
4. Test in development environment
5. Merge `dev` to `main` → auto-deploys to production

## Troubleshooting

### Deployment Fails

1. Check GitHub Actions logs
2. Check Fly.io logs: `flyctl logs -a <app-name>`
3. Verify SSM parameters are set correctly
4. Verify AWS credentials have proper permissions

### Migrations Fail

1. Check migration logs: `flyctl logs -a <app-name> | grep -i migration`
2. Verify database connection string in SSM
3. Check database permissions
4. Run migrations manually to see detailed error

### App Won't Start

1. Check health endpoint
2. Review application logs
3. Verify all required SSM parameters are set
4. Check AWS credentials

## Security Notes

- Never commit secrets to git
- Use SSM Parameter Store for all secrets in production
- Use environment-specific AWS credentials with least privilege
- Enable branch protection on `main` branch
- Require pull request reviews for production deployments

