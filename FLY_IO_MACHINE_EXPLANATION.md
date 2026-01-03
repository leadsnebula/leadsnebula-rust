# Fly.io Machine Behavior Explained

## Why Machines Show "Created 3 Hours Ago"

**This is normal!** Here's what happened:

### Machine Lifecycle in Fly.io

1. **Original Machines**: When you first created the Fly.io apps, machines were created
2. **Machine Replacement**: When we set up separate dev/prod environments, Fly.io created new machines
3. **"Created" Timestamp**: Shows when the **current machine** was created, not when the app was created

### What Happened

- **Before**: You likely had machines in a single app (maybe `leadsnebula-rust`)
- **After**: We created separate apps:
  - `leadsnebula-rust-dev` (development)
  - `leadsnebula-rust` (production)
- **Result**: New machines were created for each app, showing "created 3 hours ago"

### Does This Affect Frontend?

**No!** The frontend UI application is completely separate:
- Frontend: Deployed on Vercel (`carina-frontend.vercel.app` or `dashboard.leadsnebula.com`)
- Backend API: Deployed on Fly.io (`leadsnebula-rust-dev.fly.dev` or `leadsnebula-rust.fly.dev`)

The frontend makes HTTP requests to the backend API. As long as the API URL is correct, the frontend doesn't care about Fly.io machine IDs or creation times.

## Why Machines Shut Down (Idle Timeout)

### Default Behavior

Fly.io automatically stops machines when they're idle to save costs:
- **Default idle timeout**: ~10 minutes of no traffic
- **Your machines shut down**: After 10 minutes of no requests

### Why They Don't Auto-Start During CI/Deployment

**This is a known limitation:**

1. **GitHub Actions deploys** → Triggers `flyctl deploy`
2. **Fly.io builds Docker image** → Takes 5-10 minutes
3. **Machine is stopped** → During the build, no traffic = machine stops
4. **Deployment completes** → Machine should auto-start, but there's a timing issue

**Solution**: The `auto_start_machines = true` setting should handle this, but there's a race condition during deployment.

### Current Settings

**Dev Environment** (`fly.dev.toml`):
- `auto_stop_machines = "stop"` - Stops when idle
- `auto_start_machines = true` - Should auto-start on traffic
- `min_machines_running = 0` - Can scale to zero
- `idle_timeout = "5h"` - **NEW**: Machines stay running for 5 hours

**Production Environment** (`fly.toml`):
- `auto_stop_machines = "stop"` - Stops when idle
- `auto_start_machines = true` - Should auto-start on traffic
- `min_machines_running = 1` - Always keep 1 machine running
- `idle_timeout = "3h"` - **NEW**: Machines stay running for 3 hours

## Idle Timeout Configuration

I've updated both configs:

### Dev: 5 Hours
```toml
[http_service]
  idle_timeout = "5h"  # 18000 seconds
```

### Production: 3 Hours
```toml
[http_service]
  idle_timeout = "3h"  # 10800 seconds
```

## How Idle Timeout Works

- **Before timeout**: Machine stays running even with no traffic
- **After timeout**: Machine stops to save costs
- **On next request**: Machine auto-starts (if `auto_start_machines = true`)

## Why Production Has `min_machines_running = 1`

Production is configured to always keep 1 machine running:
- **Benefit**: No cold start delay for production traffic
- **Cost**: Machine runs 24/7 (but you can stop it manually when not needed)

## Recommendations

1. **Dev**: 5-hour timeout is good for development (machines stay up during work hours)
2. **Production**: 3-hour timeout + `min_machines_running = 1` means it stays up most of the time
3. **Manual Control**: You can always start/stop machines manually:
   ```bash
   # Stop production
   flyctl machines stop <machine-id> -a leadsnebula-rust
   
   # Start production
   flyctl machines start <machine-id> -a leadsnebula-rust
   ```

## Deployment Behavior

During deployment:
1. Fly.io builds new Docker image (5-10 min)
2. Old machine continues serving traffic
3. New machine starts with new image
4. Traffic switches to new machine
5. Old machine stops

**The "created 3 hours ago" timestamp is when the current machine was created during the last successful deployment.**

