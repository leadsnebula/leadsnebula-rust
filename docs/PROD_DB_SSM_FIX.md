# Production DB URL (SSM) – Root Cause and Fix

## Exact issue

**Prod "create ping tree" returns 500** because the production API is using the **dev** Neon database.

Evidence (from direct DB checks):

- **Prod DB** (`ep-fragrant-bar`): has exactly one instance `39c83a64-2db7-4787-bb34-26e0ea8199c3`. Inserting a ping tree with that `instance_id` **succeeds**.
- **Dev DB** (`ep-bitter-frog`): does **not** have that instance; it has different instance IDs. Inserting with `39c83a64-...` **fails** with:
  `violates foreign key constraint "fk_ping_trees_instance_id" (instance_id not present in instances)`.

When the prod UI sends `instance_id: 39c83a64-...` and the prod API is connected to the **dev** DB, the insert hits the dev DB and triggers that FK error → 500.

## Root cause

SSM parameter **`/leadsnebula/prod/rust/db/connection_url`** is set to the **dev** Neon branch URL (host `ep-bitter-frog-...`) instead of the **prod** branch URL (host `ep-fragrant-bar-...`).

The app loads `DATABASE_URL` from that SSM path in production. Wrong URL → prod API talks to dev DB → create ping tree 500.

## Fix

1. **Correct SSM**  
   Set `/leadsnebula/prod/rust/db/connection_url` to the **prod** Neon branch connection string.  
   The URL host must contain **`ep-fragrant-bar`** (prod), not `ep-bitter-frog` (dev).

   Example (replace with your real prod URL from Neon):

   ```bash
   aws ssm put-parameter \
     --name "/leadsnebula/prod/rust/db/connection_url" \
     --value "postgresql://neondb_owner:PASSWORD@ep-fragrant-bar-aht6x6lv-pooler.c-3.us-east-1.aws.neon.tech/neondb?sslmode=require&channel_binding=require" \
     --type SecureString \
     --overwrite
   ```

2. **Restart prod**  
   Restart the prod app (e.g. Fly.io machines) so it picks up the new SSM value, or rely on next deploy.

## CI/CD safeguard

- **`scripts/validate-prod-db-ssm.sh`**  
  - Reads `/leadsnebula/prod/rust/db/connection_url` from SSM.  
  - Fails if the URL contains `ep-bitter-frog` (dev).  
  - Fails if the URL does not contain `ep-fragrant-bar` (prod).

- **`deploy-production.yml`**  
  - Runs this script **before** deploy when AWS credentials are available.  
  - Prevents deploying when prod SSM still points at the dev DB.

## Summary

| Environment | Neon host in URL        | Instance `39c83a64-...` in DB? |
|------------|--------------------------|---------------------------------|
| Prod DB    | `ep-fragrant-bar`        | Yes                             |
| Dev DB     | `ep-bitter-frog`         | No                              |

Prod must use the prod DB URL. Fix SSM, then redeploy or restart.
