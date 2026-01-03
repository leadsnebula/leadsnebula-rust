# Quick Reference: Fly.io Environment Setup

## Environment Mapping

| Environment | Fly.io App | Config File | GitHub Branch | Workflow File |
|------------|------------|-------------|---------------|---------------|
| **Dev** | `leadsnebula-rust-dev` | `fly.dev.toml` | `dev` | `deploy-dev.yml` |
| **Prod** | `leadsnebula-rust` | `fly.toml` | `main` | `deploy-production.yml` |

## Key Points

### ✅ Machines Stopping is Normal
- Machines auto-stop after idle timeout (5h dev, 3h prod)
- They auto-start on first request (10-30 second delay)
- This saves costs - you only pay when machines run

### ✅ GitHub Repo Connection is Optional
- GitHub Actions use `FLY_API_TOKEN` (works without UI connection)
- Repo connection in Fly.io UI is just for visibility
- Your deployments work fine without it

### ✅ Environments Are Separated
- Different apps = different machines = different databases
- Dev workflow only deploys to dev app
- Prod workflow only deploys to prod app
- No cross-contamination possible

## Common Commands

### Check Machine Status
```bash
# Using Fly.io CLI
flyctl status -a leadsnebula-rust-dev
flyctl status -a leadsnebula-rust

# Or check in Fly.io dashboard
# Dashboard → Apps → Click app → Machines tab
```

### Manually Start a Machine
```bash
flyctl machine start <machine-id> -a leadsnebula-rust-dev
```

### View Recent Deployments
```bash
flyctl releases -a leadsnebula-rust-dev
flyctl releases -a leadsnebula-rust
```

### Check App Configuration
```bash
# View dev config
cat fly.dev.toml

# View prod config
cat fly.toml
```

## Machine Behavior Explained

### Dashboard Shows "2 machines" / "1 machine"
- This is the **total number of machines created**
- Not the number currently running

### Machines Tab Shows "Stopped"
- Machines are idle and stopped (normal)
- They will auto-start on first request
- Takes 10-30 seconds (cold start)

### Why Machines Stop
- `auto_stop_machines = "stop"` in config
- Saves money when not in use
- Production has `min_machines_running = 1` so it stays up
- Dev has `min_machines_running = 0` so it can stop

## Deployment Flow

### Development Deployment
```
1. Push to `dev` branch
   ↓
2. GitHub Actions triggers `deploy-dev.yml`
   ↓
3. Deploys to `leadsnebula-rust-dev` app
   ↓
4. Uses `fly.dev.toml` config
   ↓
5. Connects to dev database
```

### Production Deployment
```
1. Push to `main` branch
   ↓
2. GitHub Actions triggers `deploy-production.yml`
   ↓
3. Deploys to `leadsnebula-rust` app
   ↓
4. Uses `fly.toml` config
   ↓
5. Connects to production database
```

## Troubleshooting

### "Machines keep stopping"
✅ **This is expected!** Machines auto-stop when idle to save costs.

### "GitHub repo not connected"
✅ **Not required!** GitHub Actions work via API token, not UI connection.

### "How does dev deploy to both?"
❌ **It doesn't!** Each workflow deploys to its own app only.

### "Machines show stopped in dashboard"
✅ **Normal!** They auto-start on first request.

## Need Help?

See detailed guide: `FLY_IO_ENVIRONMENT_SETUP.md`

