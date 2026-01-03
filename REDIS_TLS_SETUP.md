# Redis TLS/SSL Configuration

## Upstash Redis TLS Support

**Important**: Upstash Redis **ALWAYS has TLS/SSL enabled** by default. Even if the dashboard shows "TLS/SSL: Disabled", the Redis server supports TLS connections.

The dashboard display may be misleading - it might show the status for a specific connection method or be a display bug. However, Upstash documentation confirms that TLS is always available and cannot be disabled.

## Connection String Format

### Without TLS (NOT RECOMMENDED)
```
redis://default:password@host:port
```

### With TLS (RECOMMENDED)
```
rediss://default:password@host:port
```

Note: `rediss://` (with double 's') indicates TLS/SSL encryption.

## Your Current Connection String

Your Upstash Redis connection string:
```
redis://default:705f0617c9b84bb6960c65ee5c85b638@fly-leadsnebula-redis.upstash.io:6379
```

## Recommendation: Enable TLS

**Change to:**
```
rediss://default:705f0617c9b84bb6960c65ee5c85b638@fly-leadsnebula-redis.upstash.io:6379
```

## Why Use TLS?

1. **Encryption in transit**: Protects data from interception
2. **Authentication**: Verifies you're connecting to the real Upstash server
3. **Best practice**: Industry standard for production databases

## Fly.io Note

While Fly.io uses WireGuard for internal encryption, using TLS at the application level provides:
- Defense in depth (multiple layers of security)
- Protection if traffic leaves Fly.io network
- Compliance with security standards

## How to Update

### Option 1: Update in SSM Parameter Store (Recommended - No AWS CLI needed)

Use the Rust utility script:

```bash
cd /home/badinoff/projects/leadsNebula/rust

# Set AWS credentials first
export AWS_ACCESS_KEY_ID="your-key"
export AWS_SECRET_ACCESS_KEY="your-secret"
export AWS_REGION="us-east-1"

# Update with TLS enabled (default)
cargo run --bin update-redis-url -- --env dev --url "rediss://default:705f0617c9b84bb6960c65ee5c85b638@fly-leadsnebula-redis.upstash.io:6379"

# Or if you have REDIS_URL env var set
cargo run --bin update-redis-url -- --env dev
```

### Option 2: Update in SSM Parameter Store (Using AWS CLI)
```bash
# Update the Redis URL in SSM
aws ssm put-parameter \
  --name "/leadsnebula/dev/rust/redis/connection_url" \
  --value "rediss://default:705f0617c9b84bb6960c65ee5c85b638@fly-leadsnebula-redis.upstash.io:6379" \
  --type SecureString \
  --overwrite
```

### Option 3: Update in Fly.io Secrets
```bash
flyctl secrets set REDIS_URL="rediss://default:705f0617c9b84bb6960c65ee5c85b638@fly-leadsnebula-redis.upstash.io:6379" -a leadsnebula-rust-dev
```

## Testing TLS Connection

The Rust `redis` crate automatically handles TLS when using `rediss://` URLs. No code changes needed!

## Verification

After updating, check logs for:
```
Redis connection established successfully
```

If you see connection errors, verify:
1. The URL uses `rediss://` (not `redis://`)
2. The password is correct
3. Upstash database allows TLS connections (default: yes)

