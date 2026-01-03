# CI/CD Pipeline Explained

## 🎯 What We Built

We've set up an **automated deployment pipeline** that:
- ✅ Tests your code automatically
- ✅ Builds your application
- ✅ Deploys to the right environment based on which branch you push to
- ✅ Runs database migrations automatically

## 🏗️ Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    GitHub Repository                        │
│  leadsnebula/leadsnebula-rust                               │
└─────────────────────┬───────────────────────────────────────┘
                      │
        ┌─────────────┼─────────────┐
        │             │             │
        ▼             ▼             ▼
     dev branch   staging branch  main branch
        │             │             │
        │             │             │
        ▼             ▼             ▼
┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│   Dev Env   │ │ Staging Env │ │ Production │
│  Fly.io     │ │  Fly.io     │ │   Fly.io   │
│             │ │             │ │             │
│ Database:   │ │ Database:   │ │ Database:   │
│ Dev DB      │ │ Staging DB │ │ Production  │
│             │ │             │ │    DB       │
└─────────────┘ └─────────────┘ └─────────────┘
```

## 📋 The Three Environments

### 1. **Development** (`dev` branch)
- **Fly.io App:** `leadsnebula-rust-dev`
- **Database:** Development database branch
- **Purpose:** Test new features before they go to staging
- **Auto-deploys:** Every time you push to `dev` branch
- **Resources:** 512MB RAM, can scale to zero (cheaper)

### 2. **Staging** (`staging` branch)
- **Fly.io App:** `leadsnebula-rust-staging`
- **Database:** Staging database branch
- **Purpose:** Final testing before production
- **Auto-deploys:** Every time you push to `staging` branch
- **Resources:** 1GB RAM, always running

### 3. **Production** (`main` branch)
- **Fly.io App:** `leadsnebula-rust`
- **Database:** Production database
- **Purpose:** Live application serving real users
- **Auto-deploys:** Every time you push to `main` branch
- **Resources:** 1GB RAM, always running, monitored

## 🔄 How the Deployment Process Works

### Step-by-Step Flow

```
1. You write code
   ↓
2. You commit and push to a branch
   ↓
3. GitHub Actions automatically triggers
   ↓
4. CI Workflow runs:
   - ✅ Checks code formatting
   - ✅ Runs linter (clippy)
   - ✅ Runs tests
   - ✅ Builds the project
   - ✅ Builds Docker image
   ↓
5. If CI passes, Deployment Workflow runs:
   - ✅ Connects to Fly.io
   - ✅ Builds Docker image on Fly.io
   - ✅ Runs database migrations (release_command)
   - ✅ Deploys new version
   - ✅ Health check verifies it's working
   ↓
6. Your app is live! 🎉
```

## 🔧 The Workflows Explained

### 1. **CI Workflow** (`.github/workflows/ci.yml`)

**When it runs:**
- Every push to `dev`, `staging`, or `main`
- Every pull request

**What it does:**
1. **Check formatting** - Makes sure code is properly formatted
2. **Run clippy** - Catches common Rust mistakes and style issues
3. **Run tests** - Executes all your test suite
4. **Build project** - Compiles everything to make sure it works
5. **Build Docker image** - Creates the Docker image (but doesn't push it)

**Purpose:** Catch problems **before** deployment

### 2. **Deploy to Dev** (`.github/workflows/deploy-dev.yml`)

**When it runs:**
- Every push to `dev` branch
- Can also be triggered manually

**What it does:**
1. Checks out your code
2. Installs Fly.io CLI
3. Deploys to `leadsnebula-rust-dev` using `fly.dev.toml`
4. Runs health check to verify deployment

**Config file:** `fly.dev.toml` (development-specific settings)

### 3. **Deploy to Staging** (`.github/workflows/deploy-staging.yml`)

**When it runs:**
- Every push to `staging` branch
- Can also be triggered manually

**What it does:**
- Same as dev, but deploys to `leadsnebula-rust-staging`
- Uses `fly.staging.toml` config

### 4. **Deploy to Production** (`.github/workflows/deploy-production.yml`)

**When it runs:**
- Every push to `main` branch
- Can also be triggered manually

**What it does:**
- Same as others, but deploys to `leadsnebula-rust` (production)
- Uses `fly.toml` config
- Can send notifications on failure (Slack, etc.)

## 🗄️ Database Migrations

**How it works:**
- Each `fly.toml` file has a `release_command = "/app/run-migrations"`
- When Fly.io deploys, it:
  1. Starts a temporary VM
  2. Runs `/app/run-migrations` (your migration binary)
  3. Migrations connect to the database using SSM parameters
  4. Runs all pending migrations
  5. Then starts your application

**Why this is safe:**
- Migrations run **before** the new app version starts
- If migrations fail, the old version keeps running
- Each environment has its own database, so migrations are isolated

## 🔐 Configuration Management

### How Each Environment Gets Its Config

```
┌─────────────────────────────────────────┐
│         Fly.io App Starts                │
└──────────────────┬───────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│  Reads ENVIRONMENT variable              │
│  (set in Fly.io secrets)                 │
│  - development                           │
│  - staging                                │
│  - production                             │
└──────────────────┬───────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│  Connects to AWS SSM Parameter Store    │
│  Using AWS credentials (from secrets)   │
└──────────────────┬───────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│  Loads config from SSM paths:           │
│  /leadsnebula/{env}/rust/db/...         │
│  /leadsnebula/{env}/rust/jwt/...        │
│  /leadsnebula/{env}/rust/sentry/...     │
└──────────────────┬───────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│  Application starts with correct config  │
└─────────────────────────────────────────┘
```

## 🚀 Typical Development Workflow

### Scenario: Adding a New Feature

```
1. Create feature branch from dev
   git checkout dev
   git checkout -b feature/new-api-endpoint

2. Make your changes
   # Edit code, add features, etc.

3. Test locally
   cargo test
   cargo run --bin leadsnebula-api

4. Commit and push
   git add .
   git commit -m "Add new API endpoint"
   git push origin feature/new-api-endpoint

5. Create Pull Request to dev branch
   # GitHub will run CI automatically
   # Review code, merge when ready

6. Merge to dev → Auto-deploys to development
   # Test in dev environment
   # Verify everything works

7. Merge dev to staging → Auto-deploys to staging
   # Final testing before production
   # Get stakeholder approval

8. Merge staging to main → Auto-deploys to production
   # Your feature is now live! 🎉
```

## 🔍 What Happens During Deployment

### When You Push to `dev` Branch:

```
1. GitHub Actions detects push
   ↓
2. CI workflow runs (tests, linting, build)
   ↓
3. If CI passes, "Deploy to Dev" workflow starts
   ↓
4. Fly.io receives deployment request
   ↓
5. Fly.io builds Docker image from your code
   ↓
6. Fly.io runs release_command:
   - Executes: /app/run-migrations
   - Connects to dev database
   - Runs pending migrations
   ↓
7. Fly.io starts new app version
   ↓
8. Health check verifies app is responding
   ↓
9. Old version is stopped
   ↓
10. Deployment complete!
```

### Rollback Process

If something goes wrong:

```bash
# List recent deployments
flyctl releases -a leadsnebula-rust-dev

# Rollback to previous version
flyctl releases rollback <release-id> -a leadsnebula-rust-dev
```

## 📊 Monitoring & Observability

### Health Checks
- Each environment has automatic health checks
- Endpoint: `/health` or `/api/health`
- Fly.io checks every 10-30 seconds
- If health check fails, Fly.io restarts the app

### Error Tracking
- **Sentry** automatically captures:
  - Panics
  - Errors
  - Performance issues
- Each environment sends to the same Sentry project
- You can filter by environment in Sentry dashboard

### Logs
```bash
# View logs for any environment
flyctl logs -a leadsnebula-rust-dev
flyctl logs -a leadsnebula-rust-staging
flyctl logs -a leadsnebula-rust
```

## 🔒 Security Features

1. **Secrets Management:**
   - All secrets stored in AWS SSM Parameter Store
   - Encrypted at rest
   - Environment-specific (dev can't access prod secrets)

2. **Database Isolation:**
   - Each environment has its own database
   - No cross-environment data access

3. **Authentication:**
   - Fly.io API token stored as GitHub secret
   - AWS credentials stored as Fly.io secrets
   - Never committed to git

## 🎯 Key Benefits

1. **Automated:** Push code → It deploys automatically
2. **Safe:** Tests run before deployment
3. **Isolated:** Each environment is separate
4. **Fast:** Migrations run automatically
5. **Observable:** Errors tracked in Sentry
6. **Rollback:** Easy to revert if needed

## 📝 Configuration Files

- **`.github/workflows/ci.yml`** - CI pipeline (tests, linting)
- **`.github/workflows/deploy-dev.yml`** - Dev deployment
- **`.github/workflows/deploy-staging.yml`** - Staging deployment
- **`.github/workflows/deploy-production.yml`** - Production deployment
- **`fly.dev.toml`** - Dev environment config
- **`fly.staging.toml`** - Staging environment config
- **`fly.toml`** - Production environment config

## 🚨 What Happens If Something Fails?

### CI Fails:
- Deployment **doesn't happen**
- You fix the issue and push again
- Example: Test fails → Fix test → Push → CI passes → Deploys

### Deployment Fails:
- Old version keeps running
- You can see error in GitHub Actions logs
- Fix the issue and push again
- Example: Migration fails → Fix migration → Push → Deploys

### App Crashes After Deployment:
- Fly.io health checks detect it
- Fly.io automatically restarts the app
- Errors sent to Sentry
- You can rollback if needed

## 🎓 Summary

**In simple terms:**
- Push to `dev` → Auto-deploys to development
- Push to `staging` → Auto-deploys to staging  
- Push to `main` → Auto-deploys to production

**Each deployment:**
1. Runs tests first (CI)
2. Builds your app
3. Runs database migrations
4. Deploys to Fly.io
5. Verifies it's working

**Everything is automated** - you just push code and it happens! 🚀

