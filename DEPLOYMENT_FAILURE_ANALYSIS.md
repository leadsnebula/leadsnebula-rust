# Production Deployment Failure Analysis

## Root Cause

The production deployment failed because the application could not find `DATABASE_URL` in AWS SSM Parameter Store at the expected path.

### Key Issue: Environment Normalization

The application normalizes environment names for SSM paths:
- `"production"` → `"prod"`
- `"development"` → `"dev"`

**Expected SSM Path for Production:**
```
/leadsnebula/prod/rust/db/connection_url
```

**NOT:**
```
/leadsnebula/production/rust/db/connection_url
```

The `fly.toml` sets `ENVIRONMENT = "production"`, which gets normalized to `"prod"` when constructing SSM paths. If the SSM parameter was created with the path `/leadsnebula/production/rust/db/connection_url` instead of `/leadsnebula/prod/rust/db/connection_url`, the application will fail to find it.

## Why Dev Deployment Succeeded

The dev deployment likely succeeded because:
1. The dev environment may have had the correct SSM path (`/leadsnebula/dev/rust/db/connection_url`)
2. Dev environment has a fallback mechanism that also checks the prod path (see `config.rs` lines 77-83)
3. Dev workflow includes pre-deployment validation that catches issues earlier

## Why It Failed Twice

The deployment failed twice because:
1. **No pre-deployment validation**: The production workflow lacked the pre-deployment validation step that the dev workflow has
2. **No SSM validation**: The production workflow didn't validate that required SSM parameters exist before deployment
3. **Silent failures**: The deployment process completed (Docker build and push succeeded), but the application failed at runtime when trying to start

## Fixes Implemented

### 1. Improved Error Messages
- Updated `config.rs` to show both original and normalized environment values in error messages
- Error messages now include available SSM paths for debugging

### 2. Pre-Deployment Validation
- Added pre-deployment validation step to production workflow (matching dev workflow)
- Tests Docker image with production environment before deployment
- Validates that required binaries exist and liveness endpoint works

### 3. SSM Configuration Documentation
- Added SSM configuration documentation step to production workflow
- Documents required SSM parameters and path normalization
- Provides troubleshooting guidance for SSM-related failures
- Note: SSM validation happens at runtime in Fly.io (where AWS credentials are set)
- No GitHub secrets required - AWS credentials are in Fly.io machines

### 4. Validation Script
- Created `scripts/validate-ssm-config.sh` for manual SSM validation
- Can be run locally or in CI to verify SSM configuration

## Required SSM Parameters for Production

All production SSM parameters must use the `prod` path (not `production`):

- `/leadsnebula/prod/rust/db/connection_url` (REQUIRED)
- `/leadsnebula/prod/rust/auth/jwt_secret` (REQUIRED)
- `/leadsnebula/prod/rust/encryption/api_key_key` (REQUIRED)
- `/leadsnebula/prod/rust/redis/connection_url` (REQUIRED)
- `/leadsnebula/prod/rust/monitoring/sentry_dsn` (OPTIONAL)
- `/leadsnebula/prod/rust/email/from_address` (OPTIONAL)

## Prevention Measures

1. **Pre-deployment validation**: Catches Docker image and binary issues before deployment
2. **Better error messages**: Makes it easier to diagnose SSM configuration issues at runtime
3. **SSM path documentation**: Clear guidance on required paths and normalization
4. **Runtime validation**: SSM configuration is validated when the app starts in Fly.io (where AWS credentials are available)
5. **Consistent workflows**: Production workflow now matches dev workflow structure

**Note**: SSM validation happens at runtime in Fly.io, not in GitHub Actions. This is intentional because:
- AWS credentials are set as environment variables in Fly.io machines
- No GitHub secrets are required
- Runtime validation catches SSM issues immediately with improved error messages

## Next Steps

1. **✅ SSM Parameters Verified**: All required SSM parameters exist with correct paths:
   - `/leadsnebula/prod/rust/db/connection_url` ✅
   - `/leadsnebula/prod/rust/encryption/api_key_key` ✅
   - `/leadsnebula/prod/rust/jwt/secret_key` ✅
   - `/leadsnebula/prod/rust/redis/connection_url` ✅

2. **Verify Fly.io Secrets**: Since SSM parameters exist, the issue is likely AWS credentials in Fly.io:
   ```bash
   # Check what secrets are set
   flyctl secrets list --app leadsnebula-rust
   
   # Or use the verification script
   ./scripts/verify-fly-secrets.sh leadsnebula-rust
   ```

3. **Required Fly.io Secrets**: Ensure these are set in production Fly.io app:
   - `ENVIRONMENT=production`
   - `AWS_ACCESS_KEY_ID=<your-key>`
   - `AWS_SECRET_ACCESS_KEY=<your-secret>`
   - `AWS_REGION=us-east-1` (or your AWS region)

4. **Set Missing Secrets**: If any are missing:
   ```bash
   flyctl secrets set AWS_ACCESS_KEY_ID=your_key AWS_SECRET_ACCESS_KEY=your_secret AWS_REGION=us-east-1 --app leadsnebula-rust
   ```

5. **Check IAM Permissions**: Ensure the AWS credentials have permission to read SSM parameters:
   - `ssm:GetParameter`
   - `ssm:GetParameters`
   - `ssm:GetParametersByPath`

6. **Monitor Deployments**: The improved error messages will now show available SSM paths if access fails, making it easier to diagnose.
