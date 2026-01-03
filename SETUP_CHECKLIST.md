# CI/CD Setup Checklist

Use this checklist to set up your complete CI/CD pipeline.

## ✅ Initial Setup (One-time)

### 1. Fly.io Apps
- [ ] Create dev app: `flyctl apps create leadsnebula-rust-dev`
- [ ] Verify production app exists: `flyctl apps list | grep leadsnebula-rust`

Or run the setup script:
```bash
./scripts/setup-environments.sh
```

### 2. GitHub Secrets
- [ ] Go to: https://github.com/YOUR_ORG/YOUR_REPO/settings/secrets/actions
- [ ] Add `FLY_API_TOKEN` (get it with: `flyctl auth token`)
- [ ] (Optional) Add `SLACK_WEBHOOK_URL` for production notifications

### 3. AWS SSM Parameters

For each environment (development, production):

- [ ] Database URL: `/leadsnebula/{env}/rust/db/connection_url`
- [ ] JWT Secret: `/leadsnebula/{env}/rust/jwt/secret_key`
- [ ] (Optional) Sentry DSN: `/leadsnebula/{env}/rust/sentry/dsn`
- [ ] (Optional) Redis URL: `/leadsnebula/{env}/rust/redis/connection_url`

Run the setup script for each:
```bash
ENVIRONMENT=development ./scripts/setup-ssm-parameters.sh
ENVIRONMENT=production ./scripts/setup-ssm-parameters.sh
```

### 4. Fly.io Secrets

Set AWS credentials for each app:

```bash
# Development
flyctl secrets set -a leadsnebula-rust-dev \
  ENVIRONMENT=development \
  AWS_REGION=us-east-1 \
  AWS_ACCESS_KEY_ID=your-key \
  AWS_SECRET_ACCESS_KEY=your-secret

# Production
flyctl secrets set -a leadsnebula-rust \
  ENVIRONMENT=production \
  AWS_REGION=us-east-1 \
  AWS_ACCESS_KEY_ID=your-key \
  AWS_SECRET_ACCESS_KEY=your-secret
```

### 5. Initial Deployments

Deploy each environment once to verify everything works:

```bash
# Development
flyctl deploy --config fly.dev.toml -a leadsnebula-rust-dev

# Production
flyctl deploy --config fly.toml -a leadsnebula-rust
```

### 6. Verify Deployments

Check health endpoints:
- [ ] Dev: https://leadsnebula-rust-dev.fly.dev/health
- [ ] Production: https://leadsnebula-rust.fly.dev/health

## ✅ Branch Setup

- [ ] Ensure `dev` branch exists
- [ ] Ensure `main` branch exists (production)
- [ ] Set up branch protection rules (optional but recommended for `main`)

## ✅ GitHub Actions

After pushing the workflows, verify they appear:
- [ ] Go to: https://github.com/YOUR_ORG/YOUR_REPO/actions
- [ ] Verify workflows are listed:
  - CI
  - Deploy to Dev
  - Deploy to Production

## ✅ Test the Pipeline

1. [ ] Make a small change to `dev` branch
2. [ ] Push to `dev` → Should trigger CI and auto-deploy to dev
3. [ ] Verify deployment succeeded
4. [ ] Merge `dev` to `main` → Should auto-deploy to production
5. [ ] Verify production deployment

## ✅ Monitoring Setup

- [ ] Set up Sentry for each environment
- [ ] Configure alerts in Sentry
- [ ] Set up Fly.io monitoring/alerts (optional)
- [ ] Bookmark log viewing commands:
  ```bash
  flyctl logs -a leadsnebula-rust-dev
  flyctl logs -a leadsnebula-rust
  ```

## 🎉 You're Done!

Your CI/CD pipeline is now set up. Every push will:
- Run tests and linting
- Build the Docker image
- Deploy to the appropriate environment
- Run database migrations automatically

See [DEPLOYMENT.md](./DEPLOYMENT.md) for detailed documentation.

