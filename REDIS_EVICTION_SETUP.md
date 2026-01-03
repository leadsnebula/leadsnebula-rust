# Enable Redis Eviction

You selected "No" for eviction when creating the Redis database, but you want to enable it.

## Enable Eviction via Fly.io CLI

Run this command to enable eviction on your Redis database:

```bash
flyctl redis update leadsnebula-redis --eviction true
```

## What Eviction Does

When eviction is enabled:
- Redis can automatically remove old keys when memory is full
- This is useful for caching scenarios where you want Redis to manage memory automatically
- Prevents Redis from running out of memory and crashing

## Alternative: Manual Memory Management

If you prefer to manage memory manually:
- Set appropriate TTLs on all cached keys (which we're already doing)
- Monitor Redis memory usage
- Manually delete keys when needed

For caching use cases, **eviction is recommended** as it provides automatic memory management.

