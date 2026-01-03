# Fly.io Environment Setup Guide

## Understanding Your Current Setup

### Current Configuration

**Development Environment:**
- **App Name:** `leadsnebula-rust-dev`
- **Config File:** `fly.dev.toml`
- **GitHub Branch:** `dev`
- **Workflow:** `.github/workflows/deploy-dev.yml`
- **Auto-stop:** Enabled (saves costs)
- **Min Machines:** 0 (can scale to zero)

**Production Environment:**
- **App Name:** `leadsnebula-rust`
- **Config File:** `fly.toml`
- **GitHub Branch:** `main`
- **Workflow:** `.github/workflows/deploy-production.yml`
- **Auto-stop:** Enabled (but min_machines_running = 1)
- **Min Machines:** 1 (always running)

## Issue #1: Machines Keep Shutting Down

### Why This Happens

Your machines are configured with:
```toml
auto_stop_machines = "stop"
auto_start_machines = true
idle_timeout = "5h"  # Dev: 5 hours, Prod: 3 hours
```

**This is EXPECTED behavior:**
- Machines automatically stop after being idle for the timeout period
- They automatically start when they receive a request
- This saves costs (you only pay when machines are running)

### Understanding Machine States

1. **Dashboard shows "2 machines" / "1 machine"** - This is the total number of machines created
2. **When you click into the app, machines show as "stopped"** - This means they're idle and stopped
3. **Machines auto-start on first request** - Takes 10-30 seconds (cold start)

### Options to Keep Machines Running

**Option A: Keep Current Setup (Recommended for Dev)**
- Dev machines stop after 5 hours of inactivity
- They auto-start on first request
- Saves money, but has cold start delay

**Option B: Keep Dev Machine Always Running**
Update `fly.dev.toml`:
```toml
min_machines_running = 1  # Change from 0 to 1
```
**Cost:** ~$5-10/month per machine

**Option C: Disable Auto-Stop (Not Recommended)**
```toml
auto_stop_machines = false  # Change from "stop" to false
```
**Cost:** Machines run 24/7, you pay even when idle

## Issue #2: GitHub Repo Connection

### Understanding Two Different Deployment Methods

**Method 1: GitHub Actions (What You're Using)**
- Uses `FLY_API_TOKEN` to deploy via Fly.io API
- **Does NOT require** GitHub repo connection in Fly.io UI
- Works independently of Fly.io's UI
- Your workflows are already set up correctly

**Method 2: Fly.io UI Deployments (Optional)**
- Connects GitHub repo in Fly.io dashboard
- Allows deploying from Fly.io UI
- **Separate from GitHub Actions**
- Useful for visibility and manual deployments

### Why `leadsnebula-rust-dev` Doesn't Have Repo Connected

The GitHub repo connection is **optional** and **not required** for GitHub Actions to work. Your GitHub Actions workflows use the API token, so they work fine without the UI connection.

### How to Connect GitHub Repo (Optional)

1. Go to Fly.io dashboard
2. Select `leadsnebula-rust-dev` app
3. Go to **Settings** tab
4. Scroll to **"Connect this app to GitHub repository"**
5. Click **"Choose GitHub repository"**
6. Select your repository
7. Choose branch: `dev` for dev app, `main` for production app

**Note:** This is just for visibility and Fly.io's built-in deployment feature. Your GitHub Actions will continue to work the same way.

## Issue #3: Environment Separation

### Current Setup is Correct ✅

Your environments are properly separated:

```
┌─────────────────────────────────────────┐
│         GitHub Repository                │
│                                          │
│  ┌──────────┐      ┌──────────┐        │
│  │  dev     │      │  main    │        │
│  │  branch  │      │  branch  │        │
│  └────┬─────┘      └────┬─────┘        │
│       │                 │               │
│       │                 │               │
└───────┼─────────────────┼───────────────┘
        │                 │
        ▼                 ▼
┌──────────────┐   ┌──────────────┐
│ GitHub       │   │ GitHub       │
│ Actions      │   │ Actions      │
│ Workflow:    │   │ Workflow:    │
│ deploy-dev   │   │ deploy-prod  │
└──────┬───────┘   └──────┬───────┘
       │                  │
       │                  │
       ▼                  ▼
┌──────────────┐   ┌──────────────┐
│ Fly.io App:  │   │ Fly.io App:  │
│ leadsnebula- │   │ leadsnebula- │
│ rust-dev     │   │ rust         │
│              │   │              │
│ Config:      │   │ Config:      │
│ fly.dev.toml │   │ fly.toml     │
│              │   │              │
│ Database:    │   │ Database:    │
│ Dev DB       │   │ Prod DB      │
│              │   │              │
│ SSM Path:    │   │ SSM Path:    │
│ /leadsnebula/│   │ /leadsnebula/│
│ development/ │   │ production/  │
└──────────────┘   └──────────────┘
```

### How GitHub Actions Work

**Dev Workflow (`deploy-dev.yml`):**
- **Triggers:** Push to `dev` branch
- **Deploys to:** `leadsnebula-rust-dev` (only)
- **Uses config:** `fly.dev.toml`
- **Cannot deploy to production** - different app name

**Production Workflow (`deploy-production.yml`):**
- **Triggers:** Push to `main` branch
- **Deploys to:** `leadsnebula-rust` (only)
- **Uses config:** `fly.toml`
- **Cannot deploy to dev** - different app name

### Verification Steps

1. **Check workflows are separate:**
   ```bash
   # View dev workflow
   cat .github/workflows/deploy-dev.yml | grep FLY_APP
   # Should show: FLY_APP: leadsnebula-rust-dev
   
   # View production workflow
   cat .github/workflows/deploy-production.yml | grep FLY_APP
   # Should show: FLY_APP: leadsnebula-rust
   ```

2. **Check config files:**
   ```bash
   # Dev config
   cat fly.dev.toml | grep "^app ="
   # Should show: app = "leadsnebula-rust-dev"
   
   # Production config
   cat fly.toml | grep "^app ="
   # Should show: app = "leadsnebula-rust"
   ```

## Complete Setup Verification

### Step 1: Verify Fly.io Apps Exist

```bash
# Using Fly.io CLI (if installed)
flyctl apps list

# Should show:
# leadsnebula-rust-dev
# leadsnebula-rust
```

### Step 2: Verify GitHub Workflows

1. Go to: https://github.com/leadsnebula/leadsnebula-rust/actions
2. You should see:
   - **CI** workflow (runs on all branches)
   - **Deploy to Dev** workflow (runs on `dev` branch)
   - **Deploy to Production** workflow (runs on `main` branch)

### Step 3: Verify Branch Mapping

```bash
# Check current branch
git branch

# Dev branch → deploys to leadsnebula-rust-dev
# Main branch → deploys to leadsnebula-rust
```

### Step 4: Verify Environment Variables

Each app should have:
- `ENVIRONMENT=development` (for dev) or `ENVIRONMENT=production` (for prod)
- `AWS_REGION=us-east-1`
- AWS credentials (for SSM access)

### Step 5: Verify SSM Parameters

Each environment has separate SSM paths:
- Dev: `/leadsnebula/development/rust/...`
- Prod: `/leadsnebula/production/rust/...`

## Recommended Configuration

### For Development (Cost-Effective)

Keep current setup:
- `min_machines_running = 0` (scale to zero)
- `auto_stop_machines = "stop"` (stop when idle)
- `idle_timeout = "5h"` (stay running for 5 hours)
- Accept cold start delay (10-30 seconds)

### For Production (Always Available)

Current setup is good:
- `min_machines_running = 1` (always running)
- `auto_stop_machines = "stop"` (but won't stop due to min_machines)
- `idle_timeout = "3h"` (backup timeout)

## Troubleshooting

### Machines Show as "Stopped" in Dashboard

**This is normal!** Machines auto-stop when idle. They will:
1. Auto-start on first request
2. Take 10-30 seconds to start (cold start)
3. Stay running while receiving traffic
4. Stop again after idle timeout

### GitHub Actions Not Deploying

1. Check `FLY_API_TOKEN` secret is set in GitHub
2. Verify workflow triggers (branch names must match)
3. Check workflow logs in GitHub Actions

### Wrong Environment Deployed

1. Verify branch name matches workflow trigger
2. Verify `FLY_APP` env var in workflow matches app name
3. Verify config file (`fly.dev.toml` vs `fly.toml`)

## Important: Main Branch Setup

**Current Status:** Your repository only has a `dev` branch. The production workflow (`deploy-production.yml`) is configured to trigger on pushes to `main` branch, but that branch doesn't exist yet.

### When You're Ready for Production

1. **Create main branch from dev:**
   ```bash
   git checkout dev
   git pull origin dev
   git checkout -b main
   git push origin main
   ```

2. **Or create main branch from current dev:**
   ```bash
   git checkout dev
   git checkout -b main
   git push -u origin main
   ```

3. **Set main as default branch (optional):**
   - Go to GitHub repo settings
   - Branches → Default branch
   - Change from `dev` to `main`

**Note:** Until `main` branch exists, production deployments won't trigger automatically. You can still deploy manually via:
```bash
flyctl deploy --config fly.toml -a leadsnebula-rust
```

## Summary

✅ **Your setup is correct:**
- Environments are properly separated
- GitHub Actions workflows are correctly configured
- Machines auto-stopping is expected behavior (saves costs)

⚠️ **Action needed:**
- Create `main` branch when ready for production auto-deployments

✅ **Optional improvements:**
- Connect GitHub repo in Fly.io UI for visibility (not required)
- Adjust `min_machines_running` if you want dev always running

✅ **No action needed:**
- GitHub Actions work without repo connection
- Machines stopping is normal and saves money
- Environment separation is working correctly

