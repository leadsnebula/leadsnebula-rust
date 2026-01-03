# GitHub Actions Setup - Step by Step Guide

## ✅ Step 1: Workflows Are Already Created (DONE!)

The workflow files have been committed and pushed. You can **skip the template selection page** you're seeing.

## Step 2: Navigate to Your Workflows

1. **In GitHub, go to the Actions tab** (you should already be there)
2. **Click "Skip this and set up a workflow yourself →"** at the top of the page
   - OR just click on the **"Actions"** tab in the navigation bar
3. You should now see your workflows listed:
   - ✅ **CI** - Runs tests and builds
   - ✅ **Deploy to Dev** - Deploys dev branch
   - ✅ **Deploy to Staging** - Deploys staging branch  
   - ✅ **Deploy to Production** - Deploys main branch

## Step 3: Add the Fly.io API Token Secret

This is **REQUIRED** for deployments to work.

### 3a. Get Your Fly.io API Token

Run this command in your terminal:

```bash
flyctl auth token
```

Copy the token that's displayed (it will look like: `fly_xxxxxxxxxxxxxxxxxxxxx`)

### 3b. Add It to GitHub Secrets

1. **Go to your repository settings:**
   - Click on **"Settings"** tab (top navigation bar)
   - Or go directly to: `https://github.com/leadsnebula/leadsnebula-rust/settings`

2. **Navigate to Secrets:**
   - In the left sidebar, click **"Secrets and variables"**
   - Click **"Actions"**

3. **Add the secret:**
   - Click **"New repository secret"** button
   - **Name:** `FLY_API_TOKEN`
   - **Secret:** Paste the token you got from `flyctl auth token`
   - Click **"Add secret"**

✅ You should now see `FLY_API_TOKEN` in your secrets list.

## Step 4: Verify Workflows Are Set Up

1. **Go back to the Actions tab**
2. **You should see your workflows listed:**
   - CI
   - Deploy to Dev
   - Deploy to Staging
   - Deploy to Production

3. **Check if any workflows have run:**
   - If you see a workflow run (from the push we just did), click on it
   - The **CI** workflow should have run automatically when we pushed
   - Check if it completed successfully (green checkmark)

## Step 5: Test the CI Workflow

The CI workflow should have already run from our push. Let's verify:

1. **In the Actions tab**, look for a workflow run called **"CI"**
2. **Click on it** to see the details
3. **You should see:**
   - ✅ Test job (runs tests, linting, formatting)
   - ✅ Build job (builds the Docker image)

If you see any failures, let me know and we can fix them.

## Step 6: Test Deployment (Optional - Can Do Later)

Once you've set up SSM parameters, you can test a deployment:

1. **Make a small change** to any file (or just add a comment)
2. **Commit and push to `dev` branch:**
   ```bash
   git checkout dev
   # Make a small change
   git add .
   git commit -m "Test deployment"
   git push origin dev
   ```

3. **Go to Actions tab** and watch the **"Deploy to Dev"** workflow run
4. **It should:**
   - Build the Docker image
   - Deploy to `leadsnebula-rust-dev` on Fly.io
   - Run health check

## Troubleshooting

### "Workflows not showing up"
- Make sure you're on the **Actions** tab
- Refresh the page
- The workflows should appear after the push we just did

### "Deployment fails with authentication error"
- Make sure you added `FLY_API_TOKEN` secret correctly
- Verify the token is valid: `flyctl auth whoami`

### "Can't find Secrets section"
- Make sure you have admin/write access to the repository
- Go to: Settings → Secrets and variables → Actions

## What Each Workflow Does

### CI Workflow
- **Triggers:** Every push and pull request
- **Runs:** Tests, linting, formatting checks, builds the project
- **No deployment** - just validation

### Deploy to Dev
- **Triggers:** Push to `dev` branch
- **Deploys to:** `leadsnebula-rust-dev` on Fly.io
- **Config file:** `fly.dev.toml`

### Deploy to Staging
- **Triggers:** Push to `staging` branch
- **Deploys to:** `leadsnebula-rust-staging` on Fly.io
- **Config file:** `fly.staging.toml`

### Deploy to Production
- **Triggers:** Push to `main` branch
- **Deploys to:** `leadsnebula-rust` on Fly.io
- **Config file:** `fly.toml`

## Next Steps After This

1. ✅ Workflows are set up (DONE)
2. ⏳ Add `FLY_API_TOKEN` secret (DO THIS NOW)
3. ⏳ Set up SSM parameters for each environment
4. ⏳ Test a deployment

## Quick Reference

**Get Fly.io token:**
```bash
flyctl auth token
```

**Add to GitHub:**
- Settings → Secrets and variables → Actions → New repository secret
- Name: `FLY_API_TOKEN`
- Value: (paste token)

**View workflows:**
- Go to Actions tab in GitHub
- You should see all 4 workflows listed

