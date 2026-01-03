# How to Add FLY_API_TOKEN to GitHub Secrets

## Step 1: Get Your Fly.io Token

Run this command:
```bash
~/.fly/bin/flyctl auth token
```

Copy the **entire token** (it's very long, starts with `fm2_` or `fo1_`)

## Step 2: Add to GitHub Secrets

1. **Go to your repository secrets page:**
   - Direct link: https://github.com/leadsnebula/leadsnebula-rust/settings/secrets/actions
   - OR navigate: Repository → Settings → Secrets and variables → Actions

2. **Click "New repository secret"** button

3. **Fill in the form:**
   - **Name:** `FLY_API_TOKEN` (exactly this, case-sensitive)
   - **Secret:** Paste the entire token you copied
   - Click **"Add secret"**

4. **Verify it's added:**
   - You should see `FLY_API_TOKEN` in the list of secrets

## Step 3: Verify Workflows

1. **Go to Actions tab:**
   - https://github.com/leadsnebula/leadsnebula-rust/actions

2. **You should see 4 workflows:**
   - ✅ CI
   - ✅ Deploy to Dev
   - ✅ Deploy to Staging
   - ✅ Deploy to Production

3. **Click on "Deploy to Dev"** workflow
   - It should show it's waiting or has run
   - If it failed before, it will work now after adding the token

## Troubleshooting

### "I don't see the workflows"
- Make sure you're on the **Actions** tab
- If you see "New workflow" page, click **"Skip this and set up a workflow yourself →"**
- Refresh the page

### "Deployment still fails"
- Make sure the secret name is exactly `FLY_API_TOKEN` (case-sensitive)
- Make sure you copied the entire token (it's very long)
- Try re-running the workflow after adding the secret

### "Can't find Secrets section"
- Make sure you have admin/write access to the repository
- The path is: Settings → Secrets and variables → Actions

