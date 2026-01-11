# Scripts Directory

This directory contains utility scripts for database operations, infrastructure management, and development workflows.

## Script Inventory

### Database Operations

#### `analyze-query-patterns.sh`
**Purpose**: Analyze query patterns on production database to identify missing indexes and performance issues.  
**Usage**: `./scripts/analyze-query-patterns.sh [environment] [report-file]`  
**Requirements**: AWS credentials, `psql`, database access via SSM  
**Output**: Generates a query analysis report with EXPLAIN ANALYZE results

#### `audit-rls-coverage.sh`
**Purpose**: Audit Row Level Security (RLS) coverage on all tables to identify tables missing proper isolation policies.  
**Usage**: `./scripts/audit-rls-coverage.sh [environment] [report-file]`  
**Requirements**: AWS credentials, `psql`, database access via SSM  
**Output**: Generates an RLS audit report with policy coverage analysis

#### `monitor-index-usage.sh`
**Purpose**: Comprehensive monitoring of index usage to identify unused indexes and candidates for removal.  
**Usage**: `./scripts/monitor-index-usage.sh [environment] [report-file]`  
**Requirements**: AWS credentials, `psql`, database access via SSM  
**Output**: Generates a detailed index usage report

#### `verify-index-usage.sh`
**Purpose**: Quick verification of index usage for newly created indexes. For comprehensive analysis, use `monitor-index-usage.sh`.  
**Usage**: `./scripts/verify-index-usage.sh [environment] [index-name]`  
**Requirements**: AWS credentials, `psql`, database access via SSM  
**Output**: Displays index usage statistics (quick check)

### Infrastructure Management

#### `ssm-diagnostics.sh`
**Purpose**: Consolidated SSM Parameter Store diagnostics and validation. Combines validation and diagnostic capabilities.  
**Usage**: 
- `./scripts/ssm-diagnostics.sh validate <environment>` - Validate SSM configuration
- `./scripts/ssm-diagnostics.sh diagnose [environment]` - Diagnose SSM access issues  
**Requirements**: AWS credentials, AWS CLI  
**Replaces**: `validate-ssm-config.sh`, `diagnose-ssm-issue.sh`

#### `verify-fly-secrets.sh`
**Purpose**: Verify that required secrets are set in Fly.io app.  
**Usage**: `./scripts/verify-fly-secrets.sh <fly-app-name>`  
**Requirements**: `flyctl` CLI, Fly.io authentication  
**Example**: `./scripts/verify-fly-secrets.sh leadsnebula-rust`

#### `verify-neon-branches.sh`
**Purpose**: Verify Neon database branches and get connection strings. Checks for production, development, and defaults branches.  
**Usage**: `./scripts/verify-neon-branches.sh`  
**Requirements**: `neonctl` CLI, Neon authentication, `jq`  
**Output**: Lists branches and connection strings

#### `validate-refresh-db-connections.sh`
**Purpose**: Validate and refresh database connection strings. Fetches direct (non-pooled) connection strings from Neon and stores in SSM.  
**Usage**: `./scripts/validate-refresh-db-connections.sh --env <environment> [--region <region>]`  
**Requirements**: `neonctl`, AWS CLI, AWS credentials  
**Example**: `./scripts/validate-refresh-db-connections.sh --env prod`

#### `cleanup-neon-defaults-branch.sh`
**Purpose**: Cleanup Neon defaults branch with enhanced verification. Verifies defaults branch details before deletion.  
**Usage**: `./scripts/cleanup-neon-defaults-branch.sh`  
**Requirements**: `neonctl` CLI, Neon authentication, `jq`  
**Note**: One-time cleanup script. Requires confirmation before deletion.

### Development

#### `coverage.sh`
**Purpose**: Generate code coverage report locally using `cargo-llvm-cov`.  
**Usage**: `./scripts/coverage.sh`  
**Requirements**: `cargo-llvm-cov` (installed automatically if missing)  
**Output**: Generates `lcov.info` and HTML coverage report in `coverage-html/`

## Common Requirements

Most scripts require:
- **AWS credentials**: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`
- **PostgreSQL client**: `psql` (for database scripts)
- **CLI tools**: `aws`, `neonctl`, `flyctl` (as needed per script)

## Environment Normalization

Scripts automatically normalize environment names:
- `production` → `prod`
- `development` → `dev`

## Notes

- Database scripts connect via SSM Parameter Store using the path pattern: `/leadsnebula/{env}/rust/db/connection_url_direct`
- Most scripts generate reports with timestamps in the filename
- Scripts use `set -euo pipefail` for error handling
- Connection strings are masked in output for security
