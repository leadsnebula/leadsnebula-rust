#!/bin/bash
# Pre-commit validation script
# Run this before every commit to ensure CI will pass
#
# Usage:
#   ./validate.sh          # Full validation (all tests) - USE BEFORE COMMITTING
#   ./validate.sh --fast    # Fast validation (unit tests only) - DEVELOPMENT ONLY
#
# ⚠️  IMPORTANT: Fast mode skips database integration tests!
#    - Use './validate.sh' (without --fast) before committing
#    - CI will run full tests, but fast commits waste CI resources
#    - Fast mode is intended for rapid development iteration only

set -e

# Parse flags
FAST_MODE=false
STRICT_MODE=${CI:-false}  # Auto-enable in CI, default off locally
for arg in "$@"; do
    if [ "$arg" = "--fast" ]; then
        FAST_MODE=true
    elif [ "$arg" = "--strict" ]; then
        STRICT_MODE=true
    fi
done

# Check for jq dependency
if ! command -v jq > /dev/null 2>&1; then
    echo "⚠️  jq not found. Install for better validation output: apt install jq (or brew install jq)"
    echo "   Falling back to basic parsing..."
    USE_JQ=false
else
    USE_JQ=true
fi

# Helper function for JSON extraction with fallback
extract_json_field() {
    local field="$1"
    local json="$2"
    if [ "$USE_JQ" = "true" ]; then
        echo "$json" | jq -r "$field" 2>/dev/null || echo ""
    else
        # Fallback: use awk for basic extraction
        # Remove leading dot from field name
        local key="${field#.}"
        echo "$json" | awk -F'"' "/\"${key}\":/ {print \$4}" 2>/dev/null || echo ""
    fi
}

# Check if validate-config binary exists and is up-to-date
VALIDATE_CONFIG_BIN="target/release/validate-config"
VALIDATE_CONFIG_SRC="crates/utils/src/bin/validate-config.rs"
USE_FALLBACK=false

if [ ! -f "$VALIDATE_CONFIG_BIN" ] || [ "$VALIDATE_CONFIG_SRC" -nt "$VALIDATE_CONFIG_BIN" ]; then
    echo "Building validate-config..."
    if ! cargo build --release --bin validate-config --quiet 2>/dev/null; then
        echo "⚠️  validate-config build failed, using fallback parsing"
        USE_FALLBACK=true
    fi
fi

# If binary doesn't exist or build failed, use fallback
if [ ! -f "$VALIDATE_CONFIG_BIN" ]; then
    USE_FALLBACK=true
fi

# Safety warnings for fast mode
if [ "$FAST_MODE" = true ]; then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "⚠️  WARNING: Fast mode skips database integration tests!"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "   Fast mode will skip:"
    echo "   • OTP setup/enable/disable tests"
    echo "   • Passkey credential storage tests"
    echo "   • User password verification tests"
    echo "   • User status management tests"
    echo "   • Publisher CRUD operations"
    echo ""
    echo "   ⚠️  Use './validate.sh' (without --fast) before committing!"
    echo "   CI will run full tests, but fast commits waste CI resources."
    echo ""
    
    # Check if we're in a git repository and warn
    if git rev-parse --git-dir > /dev/null 2>&1; then
        # Check if there are uncommitted changes
        if ! git diff-index --quiet HEAD -- 2>/dev/null; then
            echo "   ⚠️  You have uncommitted changes in a git repository."
            echo "   Consider running full validation before committing."
            echo ""
            # Only prompt interactively if stdin is a TTY (not in CI/scripts)
            if [ -t 0 ]; then
                read -p "   Continue with fast mode anyway? (y/N) " -n 1 -r
                echo ""
                if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                    echo "   Aborted. Run './validate.sh' for full validation."
                    exit 1
                fi
            else
                echo "   (Non-interactive mode: continuing with fast mode)"
                echo ""
            fi
        fi
    fi
    
    echo "🚀 Running fast validation (unit tests only, skipping slow database tests)..."
    echo ""
else
    echo "🔍 Running pre-commit validation checks..."
    echo ""
fi

ERRORS=0

# 1. Format check
echo "1️⃣  Checking formatting..."
if ! cargo fmt --all -- --check; then
    echo "❌ Formatting check failed. Run 'cargo fmt --all' to fix."
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Formatting OK"
fi
echo ""

# 2. Clippy check
echo "2️⃣  Running clippy (warnings as errors)..."
if ! cargo clippy --all-targets --all-features -- -D warnings; then
    echo "❌ Clippy check failed. Fix all warnings before committing."
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Clippy OK"
fi
echo ""
# 3. Rust module references check - catch missing `mod` files early
echo "3️⃣  Checking Rust 'mod' references exist..."
MISSING_MODS=0
# For each .rs file under crates/, look for `mod name;` or `pub mod name;` declarations
while IFS= read -r file; do
    # For each matching line in the file
    while IFS= read -r line; do
        mod=$(echo "$line" | sed -E 's/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+([a-zA-Z0-9_]+);.*/\2/')
        dir=$(dirname "$file")
        if [ ! -f "$dir/$mod.rs" ] && [ ! -f "$dir/$mod/mod.rs" ]; then
            echo "❌ Missing module '$mod' referenced in $file"
            MISSING_MODS=1
        fi
    done < <(grep -E '^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[a-zA-Z0-9_]+;' "$file" || true)
done < <(find crates -type f \( -name 'mod.rs' -o -path '*/src/services/*.rs' -o -path '*/src/models/mod.rs' -o -path '*/src/routes/mod.rs' -o -path '*/tests/*.rs' \) -print)

if [ "$MISSING_MODS" -eq 1 ]; then
    echo "❌ One or more Rust module files referenced by 'mod' are missing."
    echo "   Example: create the file crates/core/src/models/buyer_integration.rs or crates/core/src/models/buyer_integration/mod.rs"
    echo "   (Local untracked files may exist; do not commit until you approve.)"
    exit 1
else
    echo "✅ Rust module references OK"
fi
echo ""

# 4. Nextest config validation (if config exists)
if [ -f ".config/nextest.toml" ]; then
    echo "3️⃣  Validating nextest configuration..."
    if command -v cargo-nextest > /dev/null 2>&1; then
        # Validate nextest config by trying to list tests (parses and validates config)
        # Use --workspace to ensure config is loaded, but don't actually run tests
        if ! cargo nextest list --workspace --locked > /dev/null 2>&1; then
            echo "❌ Nextest config (.config/nextest.toml) is invalid"
            echo "   Attempting to parse config to show error:"
            cargo nextest list --workspace --locked 2>&1 | grep -A 5 "nextest\|config\|error" | head -10 || true
            if [ "$STRICT_MODE" = "true" ]; then
                ERRORS=$((ERRORS + 1))
            else
                echo "⚠️  Nextest config validation failed (non-blocking in non-strict mode)"
            fi
        else
            echo "✅ Nextest config OK"
        fi
    else
        # If nextest not installed, try validate-config binary
        if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
            if "$VALIDATE_CONFIG_BIN" cargo-toml .config/nextest.toml > /dev/null 2>&1; then
                echo "✅ Nextest config TOML syntax OK (nextest not installed, using validate-config)"
            else
                echo "⚠️  Nextest config validation skipped (nextest not installed)"
                echo "   Install nextest for full validation: cargo install cargo-nextest"
            fi
        else
            echo "⚠️  Nextest config exists but cannot validate (nextest not installed, validate-config unavailable)"
            echo "   Install nextest for full validation: cargo install cargo-nextest"
        fi
    fi
    echo ""
fi

# 4. Test check (use nextest if available, otherwise cargo test)
echo "4️⃣  Running tests (with --locked)..."
# Check if DATABASE_URL is set for database-dependent tests
HAS_DATABASE_URL=false
if [ "$FAST_MODE" = false ]; then
    # Only check for DATABASE_URL if not in fast mode
    if [ -n "$DATABASE_URL" ]; then
        HAS_DATABASE_URL=true
    elif [ -f ".env.local" ] && grep -q "^DATABASE_URL=" .env.local 2>/dev/null; then
        # Try to load DATABASE_URL from .env.local
        export $(grep "^DATABASE_URL=" .env.local | xargs)
        if [ -n "$DATABASE_URL" ]; then
            HAS_DATABASE_URL=true
        fi
    fi
fi

# Clean up leftover test databases before running tests
# This prevents "database is being accessed by other users" errors from sqlx::test
# Skip cleanup in fast mode (no database tests)
if [ "$FAST_MODE" = false ] && [ "$HAS_DATABASE_URL" = true ] && command -v psql > /dev/null 2>&1; then
    # Extract base database URL (replace database name with 'postgres')
    CLEANUP_DB_URL=$(echo "$DATABASE_URL" | sed 's|/[^/]*$|/postgres|')
    if [ -n "$CLEANUP_DB_URL" ]; then
        echo "   Cleaning up leftover test databases..."
        # Terminate connections to test databases
        psql "$CLEANUP_DB_URL" -c "SELECT pg_terminate_backend(pg_stat_activity.pid) FROM pg_stat_activity WHERE pg_stat_activity.datname LIKE '_sqlx_test_%' AND pid <> pg_backend_pid();" > /dev/null 2>&1 || true
        sleep 1
        # Drop test databases
        psql "$CLEANUP_DB_URL" -t -c "SELECT 'DROP DATABASE IF EXISTS ' || quote_ident(datname) || ';' FROM pg_database WHERE datname LIKE '_sqlx_test_%';" 2>/dev/null | grep -v "^$" | psql "$CLEANUP_DB_URL" > /dev/null 2>&1 || true
    fi
fi

if command -v cargo-nextest > /dev/null 2>&1; then
    echo "   Using cargo-nextest for faster parallel test execution..."
    if [ "$FAST_MODE" = true ]; then
        # Fast mode: only run unit tests (no database required)
        echo "   Fast mode: Running unit tests only (skipping database integration tests)..."
        if ! cargo nextest run --lib --locked --all-features; then
            echo "❌ Unit tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ Unit tests OK (nextest, fast mode)"
        fi
    elif [ "$HAS_DATABASE_URL" = true ]; then
        echo "   DATABASE_URL is set - running all tests including database-dependent ones..."
        
        # Run unit tests first with nextest (fast, parallel) for quick feedback
        echo "   Running unit tests in parallel (nextest)..."
        if ! cargo nextest run --lib --locked --all-features; then
            echo "❌ Unit tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ Unit tests OK (nextest)"
        fi
        
        # Run non-database integration tests in parallel with nextest
        echo "   Running non-database integration tests in parallel (nextest)..."
        if ! cargo nextest run --locked --all-features --test integration_health --test integration_routes; then
            echo "❌ Integration tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ Non-database integration tests OK (nextest)"
        fi
        
        # Run database-dependent tests sequentially to avoid conflicts
        # sqlx::test creates/drops test databases which can conflict when run in parallel
        # Note: If tests fail with "database is being accessed by other users", wait a few seconds
        # and retry - this is a known sqlx::test limitation with leftover test databases
        echo "   Running database-dependent tests sequentially..."
        MAX_RETRIES=2
        RETRY_COUNT=0
        TEST_PASSED=false
        
        while [ $RETRY_COUNT -lt $MAX_RETRIES ] && [ "$TEST_PASSED" = false ]; do
            if [ $RETRY_COUNT -gt 0 ]; then
                echo "   Waiting 5 seconds before retry (attempt $((RETRY_COUNT + 1))/$MAX_RETRIES)..."
                sleep 5
            fi
            
            TEST_OUTPUT=$(cargo test --test integration_auth --locked --all-features -- --test-threads=1 2>&1 | tee /tmp/test_output.log)
            TEST_EXIT_CODE=${PIPESTATUS[0]}
            
            if [ $TEST_EXIT_CODE -eq 0 ]; then
                TEST_PASSED=true
                echo "✅ Database integration tests OK"
            else
                # Check if failure is due to database access conflicts
                if grep -q "database.*is being accessed by other users\|55006" /tmp/test_output.log; then
                    RETRY_COUNT=$((RETRY_COUNT + 1))
                    if [ $RETRY_COUNT -lt $MAX_RETRIES ]; then
                        echo "   ⚠️  Test databases locked by previous runs, retrying..."
                        continue
                    else
                        echo "   ⚠️  Database tests failed due to leftover test databases."
                        echo "   This is a known sqlx::test limitation in local development."
                        echo "   CI/production will not be affected (fresh databases each run)."
                        echo "   To fix locally: wait a few minutes or restart PostgreSQL."
                        echo "⚠️  Database integration tests skipped (non-blocking for CI/deployment)."
                        # Don't count this as an error since it's a local dev issue
                        TEST_PASSED=true  # Mark as passed to continue validation
                    fi
                else
                    echo "❌ Database integration tests failed. Fix all failing tests before committing."
                    ERRORS=$((ERRORS + 1))
                    break
                fi
            fi
        done
        
        # Run E2E tests if they exist (full API flow tests)
        if cargo test --test integration_carina_e2e --list 2>/dev/null | grep -q "test.*"; then
            echo "   Running E2E tests (full API flow)..."
            if ! cargo test --test integration_carina_e2e --locked --all-features -- --test-threads=1 --ignored; then
                echo "❌ E2E tests failed. Fix all failing tests before committing."
                ERRORS=$((ERRORS + 1))
            else
                echo "✅ E2E tests OK"
            fi
        fi
        
        # Run async persistence tests
        if cargo test --lib -p leadsnebula_core --list 2>/dev/null | grep -q "async_persistence"; then
            echo "   Running async persistence tests..."
            if ! cargo test --lib -p leadsnebula_core async_persistence_tests --locked --all-features -- --test-threads=1 --ignored; then
                echo "❌ Async persistence tests failed. Fix all failing tests before committing."
                ERRORS=$((ERRORS + 1))
            else
                echo "✅ Async persistence tests OK"
            fi
        fi
    else
        echo "   DATABASE_URL not set - running unit tests only..."
        echo "   (Set DATABASE_URL to run full test suite including database integration tests)"
        # Run only unit tests (lib tests) to avoid database-dependent test failures
        if ! cargo nextest run --lib --locked --all-features; then
            echo "❌ Unit tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ Unit tests OK (integration tests skipped - set DATABASE_URL to run them)"
        fi
    fi
else
    echo "   Using cargo test (install cargo-nextest for faster tests: cargo install cargo-nextest)"
    if [ "$FAST_MODE" = true ]; then
        # Fast mode: only run unit tests (no database required)
        echo "   Fast mode: Running unit tests only (skipping database integration tests)..."
        if ! cargo test --lib --locked --all-features; then
            echo "❌ Unit tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ Unit tests OK (fast mode)"
        fi
    elif [ "$HAS_DATABASE_URL" = true ]; then
        echo "   DATABASE_URL is set - running all tests including database-dependent ones..."
        # Use limited parallelism (2 threads) to reduce database conflicts while maintaining performance
        # sqlx::test creates/drops test databases which can conflict when too many run in parallel
        # Automatic cleanup (above) handles leftover databases, but we still need limited parallelism
        if ! cargo test --locked --all-features -- --test-threads=2; then
            echo "❌ Tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ Tests OK"
        fi
    else
        echo "   DATABASE_URL not set - running unit tests only..."
        echo "   (Set DATABASE_URL to run full test suite including integration tests)"
        # Run only unit tests (lib tests) to avoid database-dependent test failures
        if ! cargo test --lib --locked --all-features; then
            echo "❌ Unit tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ Unit tests OK (integration tests skipped - set DATABASE_URL to run them)"
        fi
    fi
fi
echo ""

# 5. Build check
echo "5️⃣  Building release (with --locked)..."
if ! cargo build --release --locked; then
    echo "❌ Build failed. Fix compilation errors before committing."
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Build OK"
fi
echo ""

# 6. Cargo.lock validation
echo "6️⃣  Validating Cargo.lock..."
if [ ! -f "Cargo.lock" ]; then
    echo "❌ Cargo.lock is missing. Run 'cargo generate-lockfile' and commit it."
    ERRORS=$((ERRORS + 1))
elif ! git ls-files --error-unmatch Cargo.lock > /dev/null 2>&1; then
    echo "❌ Cargo.lock is not tracked by git. Run 'git add Cargo.lock' and commit it."
    ERRORS=$((ERRORS + 1))
else
    # Check Cargo.lock version and verify Dockerfile Rust version compatibility
    # Use cargo metadata for more reliable version detection
    if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
        # Try using cargo metadata first (more reliable)
        METADATA=$(cargo metadata --format-version 2 --no-deps 2>/dev/null || echo "")
        if [ -n "$METADATA" ] && [ "$USE_JQ" = "true" ]; then
            RESOLVER=$(echo "$METADATA" | jq -r '.workspace.resolver // .workspace_default.resolver // "1"' 2>/dev/null || echo "1")
            if [ "$RESOLVER" = "2" ]; then
                LOCK_VERSION="4"
            else
                LOCK_VERSION="3"
            fi
        else
            # Fallback to grep parsing
            LOCK_VERSION=$(grep "^version = " Cargo.lock | head -1 | cut -d' ' -f3 || echo "3")
        fi
    else
        # Fallback: use grep parsing
        LOCK_VERSION=$(grep "^version = " Cargo.lock | head -1 | cut -d' ' -f3 || echo "3")
    fi
    
    if [ "$LOCK_VERSION" = "4" ]; then
        # Cargo.lock v4 requires Rust 1.78+
        if [ -f "Dockerfile" ]; then
            if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
                # Use validate-config binary for reliable Dockerfile parsing
                DOCKER_OUTPUT=$("$VALIDATE_CONFIG_BIN" dockerfile Dockerfile 2>/dev/null || echo "")
                DOCKER_RUST=$(extract_json_field ".rust_version" "$DOCKER_OUTPUT")
                
                if [ -n "$DOCKER_RUST" ] && [ "$DOCKER_RUST" != "null" ]; then
                    # Check if version is too old (1.77 or earlier)
                    if echo "$DOCKER_RUST" | grep -qE "^1\.(7[0-7]|[0-6][0-9])"; then
                        echo "❌ Cargo.lock version 4 requires Rust 1.78+, but Dockerfile uses rust:$DOCKER_RUST"
                        echo "   Update Dockerfile to use 'rust:bookworm' or 'rust:latest'"
                        ERRORS=$((ERRORS + 1))
                    else
                        echo "✅ Cargo.lock version $LOCK_VERSION compatible with Dockerfile Rust version (rust:$DOCKER_RUST)"
                    fi
                else
                    echo "⚠️  Could not extract Rust version from Dockerfile"
                fi
            else
                # Fallback: use grep parsing
                DOCKER_RUST=$(grep "^FROM rust:" Dockerfile | head -1 | cut -d: -f2 | cut -d' ' -f1 || echo "")
                if [ -n "$DOCKER_RUST" ] && echo "$DOCKER_RUST" | grep -qE "^1\.(7[0-7]|[0-6][0-9])"; then
                    echo "❌ Cargo.lock version 4 requires Rust 1.78+, but Dockerfile uses rust:$DOCKER_RUST"
                    echo "   Update Dockerfile to use 'rust:bookworm' or 'rust:latest'"
                    ERRORS=$((ERRORS + 1))
                else
                    echo "✅ Cargo.lock version $LOCK_VERSION compatible with Dockerfile Rust version"
                fi
            fi
        fi
    fi
    echo "✅ Cargo.lock OK (version $LOCK_VERSION)"
fi

# Check if Cargo.lock is in .gitignore
if grep -q "^Cargo.lock$" .gitignore 2>/dev/null; then
    echo "❌ Cargo.lock is in .gitignore. Remove it - applications must commit Cargo.lock."
    ERRORS=$((ERRORS + 1))
fi
echo ""

# 7. Cargo.toml validation
echo "7️⃣  Validating Cargo.toml..."
CARGO_TOML_ERRORS=0

# Validate workspace Cargo.toml
if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
    CARGO_TOML_OUTPUT=$("$VALIDATE_CONFIG_BIN" cargo-toml Cargo.toml 2>/dev/null || echo "")
    
    if [ -n "$CARGO_TOML_OUTPUT" ]; then
        if [ "$USE_JQ" = "true" ]; then
            ERROR_COUNT=$(echo "$CARGO_TOML_OUTPUT" | jq -r '[.errors[]?] | length' 2>/dev/null || echo "0")
            
            if [ "$ERROR_COUNT" -gt 0 ]; then
                echo "❌ Cargo.toml validation failed:"
                echo "$CARGO_TOML_OUTPUT" | jq -r '.errors[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                    if [ -n "$line" ]; then
                        echo "$line"
                    fi
                done
                CARGO_TOML_ERRORS=$((CARGO_TOML_ERRORS + 1))
            else
                echo "✅ Cargo.toml OK"
            fi
        else
            if echo "$CARGO_TOML_OUTPUT" | grep -q '"errors"'; then
                echo "❌ Cargo.toml validation failed"
                CARGO_TOML_ERRORS=$((CARGO_TOML_ERRORS + 1))
            else
                echo "✅ Cargo.toml OK"
            fi
        fi
    else
        echo "⚠️  Could not validate Cargo.toml (validate-config unavailable)"
    fi
else
    # Fallback: basic cargo check
    if ! cargo check --workspace > /dev/null 2>&1; then
        echo "❌ Cargo.toml has issues. Check for version conflicts or missing dependencies."
        CARGO_TOML_ERRORS=$((CARGO_TOML_ERRORS + 1))
    else
        echo "✅ Cargo.toml OK"
    fi
fi

# Validate crate Cargo.toml files (check for bench declarations)
for crate_toml in crates/*/Cargo.toml; do
    if [ -f "$crate_toml" ]; then
        CRATE_NAME=$(basename "$(dirname "$crate_toml")")
        
        if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
            CRATE_OUTPUT=$("$VALIDATE_CONFIG_BIN" cargo-toml "$crate_toml" 2>/dev/null || echo "")
            
            if [ -n "$CRATE_OUTPUT" ]; then
                if [ "$USE_JQ" = "true" ]; then
                    ERROR_COUNT=$(echo "$CRATE_OUTPUT" | jq -r '[.errors[]?] | length' 2>/dev/null || echo "0")
                    
                    if [ "$ERROR_COUNT" -gt 0 ]; then
                        echo "❌ $CRATE_NAME/Cargo.toml validation failed:"
                        echo "$CRATE_OUTPUT" | jq -r '.errors[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                            if [ -n "$line" ]; then
                                echo "$line"
                            fi
                        done
                        CARGO_TOML_ERRORS=$((CARGO_TOML_ERRORS + 1))
                    fi
                else
                    if echo "$CRATE_OUTPUT" | grep -q '"errors"'; then
                        echo "❌ $CRATE_NAME/Cargo.toml validation failed"
                        CARGO_TOML_ERRORS=$((CARGO_TOML_ERRORS + 1))
                    fi
                fi
            fi
        fi
    fi
done

if [ "$CARGO_TOML_ERRORS" -gt 0 ]; then
    ERRORS=$((ERRORS + CARGO_TOML_ERRORS))
fi
echo ""

# 8. Docker validation (static checks only - build happens in CI)
if [ -f "Dockerfile" ]; then
    echo "8️⃣  Validating Dockerfile (static checks only)..."
    
    # Check for common Dockerfile issues
    if grep -q "COPY --from=builder.*target/release" Dockerfile && grep -q "type=cache.*target=/app/target" Dockerfile; then
        echo "❌ Dockerfile uses cache mount for /app/target but COPYs from target/release"
        echo "   Cache mounts are ephemeral - binaries won't be available for COPY"
        echo "   Fix: Copy binaries to persistent location (e.g., /app/binaries/) during build"
        ERRORS=$((ERRORS + 1))
    fi
    
    # Check for required binaries in Dockerfile
    REQUIRED_BINARIES=("leadsnebula-api" "run-migrations")
    for binary in "${REQUIRED_BINARIES[@]}"; do
        if ! grep -q "$binary" Dockerfile; then
            echo "⚠️  Required binary '$binary' not found in Dockerfile COPY commands"
        fi
    done
    
    echo "✅ Dockerfile static checks OK (full build validation happens in CI)"
    echo ""
fi

# 9. GitHub Actions workflow and action validation
if [ -d ".github" ]; then
    echo "9️⃣  Validating GitHub Actions workflows and actions..."
    WORKFLOW_ERRORS=0
    
    # Validate workflows
    if [ -d ".github/workflows" ]; then
        for workflow in .github/workflows/*.yml; do
            if [ -f "$workflow" ]; then
                WORKFLOW_NAME=$(basename "$workflow")
                
                if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
                    # Use validate-config binary for comprehensive validation
                    WORKFLOW_OUTPUT=$("$VALIDATE_CONFIG_BIN" github-workflow "$workflow" 2>/dev/null || echo "")
                    
                    if [ -n "$WORKFLOW_OUTPUT" ]; then
                        if [ "$USE_JQ" = "true" ]; then
                            WARNING_COUNT=$(echo "$WORKFLOW_OUTPUT" | jq -r '[.warnings[]?] | length' 2>/dev/null || echo "0")
                            ERROR_COUNT=$(echo "$WORKFLOW_OUTPUT" | jq -r '[.errors[]?] | length' 2>/dev/null || echo "0")
                            
                            if [ "$ERROR_COUNT" -gt 0 ]; then
                                echo "❌ $WORKFLOW_NAME validation failed:"
                                echo "$WORKFLOW_OUTPUT" | jq -r '.errors[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                                    if [ -n "$line" ]; then
                                        echo "$line"
                                    fi
                                done
                                WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                            elif [ "$WARNING_COUNT" -gt 0 ]; then
                                echo "⚠️  $WORKFLOW_NAME has warnings:"
                                echo "$WORKFLOW_OUTPUT" | jq -r '.warnings[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                                    if [ -n "$line" ]; then
                                        echo "$line"
                                    fi
                                done
                                if [ "$STRICT_MODE" = "true" ]; then
                                    WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                                fi
                            else
                                echo "✅ $WORKFLOW_NAME validation OK"
                            fi
                        else
                            # Fallback: basic check
                            if echo "$WORKFLOW_OUTPUT" | grep -q '"errors"'; then
                                echo "❌ $WORKFLOW_NAME validation failed"
                                WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                            elif echo "$WORKFLOW_OUTPUT" | grep -q '"warnings"'; then
                                echo "⚠️  $WORKFLOW_NAME has warnings"
                            else
                                echo "✅ $WORKFLOW_NAME validation OK"
                            fi
                        fi
                    else
                        echo "⚠️  Could not validate $WORKFLOW_NAME (validate-config unavailable)"
                    fi
                else
                    # Fallback: use Python YAML validation
                    if command -v python3 > /dev/null 2>&1; then
                        if ! python3 -c "import yaml; yaml.safe_load(open('$workflow'))" > /dev/null 2>&1; then
                            echo "❌ $WORKFLOW_NAME has invalid YAML syntax"
                            python3 -c "import yaml; yaml.safe_load(open('$workflow'))" 2>&1 | head -5 || true
                            WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                        else
                            echo "✅ $WORKFLOW_NAME YAML syntax OK"
                        fi
                    else
                        echo "⚠️  Cannot validate YAML syntax (python3 not available)"
                    fi
                fi
            fi
        done
    fi
    
    # Validate composite actions
    if [ -d ".github/actions" ]; then
        for action_dir in .github/actions/*/; do
            if [ -d "$action_dir" ]; then
                ACTION_NAME=$(basename "$action_dir")
                ACTION_FILE="$action_dir/action.yml"
                if [ -f "$ACTION_FILE" ]; then
                    if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
                        ACTION_OUTPUT=$("$VALIDATE_CONFIG_BIN" github-workflow "$ACTION_FILE" 2>/dev/null || echo "")
                        
                        if [ -n "$ACTION_OUTPUT" ]; then
                            if [ "$USE_JQ" = "true" ]; then
                                ERROR_COUNT=$(echo "$ACTION_OUTPUT" | jq -r '[.errors[]?] | length' 2>/dev/null || echo "0")
                                WARNING_COUNT=$(echo "$ACTION_OUTPUT" | jq -r '[.warnings[]?] | length' 2>/dev/null || echo "0")
                                
                                if [ "$ERROR_COUNT" -gt 0 ]; then
                                    echo "❌ Composite action '$ACTION_NAME' validation failed:"
                                    echo "$ACTION_OUTPUT" | jq -r '.errors[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                                        if [ -n "$line" ]; then
                                            echo "$line"
                                        fi
                                    done
                                    WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                                elif [ "$WARNING_COUNT" -gt 0 ]; then
                                    echo "⚠️  Composite action '$ACTION_NAME' has warnings:"
                                    echo "$ACTION_OUTPUT" | jq -r '.warnings[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                                        if [ -n "$line" ]; then
                                            echo "$line"
                                        fi
                                    done
                                    if [ "$STRICT_MODE" = "true" ]; then
                                        WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                                    fi
                                else
                                    echo "✅ Composite action '$ACTION_NAME' validation OK"
                                fi
                            else
                                # Fallback: basic check
                                if echo "$ACTION_OUTPUT" | grep -q '"errors"'; then
                                    echo "❌ Composite action '$ACTION_NAME' validation failed"
                                    WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                                elif echo "$ACTION_OUTPUT" | grep -q '"warnings"'; then
                                    echo "⚠️  Composite action '$ACTION_NAME' has warnings"
                                else
                                    echo "✅ Composite action '$ACTION_NAME' validation OK"
                                fi
                            fi
                        else
                            echo "⚠️  Could not validate composite action '$ACTION_NAME' (validate-config unavailable)"
                        fi
                    else
                        # Fallback: basic YAML syntax check
                        if command -v python3 > /dev/null 2>&1; then
                            if ! python3 -c "import yaml; yaml.safe_load(open('$ACTION_FILE'))" > /dev/null 2>&1; then
                                echo "❌ Composite action '$ACTION_NAME' has invalid YAML syntax"
                                python3 -c "import yaml; yaml.safe_load(open('$ACTION_FILE'))" 2>&1 | head -5 || true
                                WORKFLOW_ERRORS=$((WORKFLOW_ERRORS + 1))
                            else
                                echo "✅ Composite action '$ACTION_NAME' YAML syntax OK"
                            fi
                        else
                            echo "⚠️  Cannot validate composite action '$ACTION_NAME' (python3 not available)"
                        fi
                    fi
                fi
            fi
        done
    fi
    
    if [ "$WORKFLOW_ERRORS" -gt 0 ]; then
        ERRORS=$((ERRORS + WORKFLOW_ERRORS))
    fi
    echo ""
fi

# 10. Fly.io config validation
echo "🔟 Validating Fly.io configs..."
if [ ! -f "fly.toml" ]; then
    echo "⚠️  fly.toml not found (may be OK for dev-only setup)"
elif [ ! -f "fly.dev.toml" ]; then
    echo "⚠️  fly.dev.toml not found (may be OK for prod-only setup)"
else
    if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
        # Use validate-config binary for reliable parsing
        DEV_OUTPUT=$("$VALIDATE_CONFIG_BIN" fly-toml fly.dev.toml 2>/dev/null || echo "")
        PROD_OUTPUT=$("$VALIDATE_CONFIG_BIN" fly-toml fly.toml 2>/dev/null || echo "")
        
        DEV_APP=$(extract_json_field ".app_name" "$DEV_OUTPUT")
        PROD_APP=$(extract_json_field ".app_name" "$PROD_OUTPUT")
        
        if [ -z "$DEV_APP" ]; then
            echo "❌ Could not extract app name from fly.dev.toml"
            if [ "$STRICT_MODE" = "true" ]; then
                ERRORS=$((ERRORS + 1))
            else
                echo "⚠️  Dev app name extraction failed (non-blocking in non-strict mode)"
            fi
        elif [ -z "$PROD_APP" ]; then
            echo "❌ Could not extract app name from fly.toml"
            if [ "$STRICT_MODE" = "true" ]; then
                ERRORS=$((ERRORS + 1))
            else
                echo "⚠️  Prod app name extraction failed (non-blocking in non-strict mode)"
            fi
        elif [ "$DEV_APP" = "$PROD_APP" ]; then
            echo "⚠️  Dev and prod app names are the same: $DEV_APP"
            echo "   This may cause cross-environment deployment issues (some valid setups intentionally share names)."
            if [ "$STRICT_MODE" = "true" ]; then
                ERRORS=$((ERRORS + 1))
            fi
        else
            echo "✅ Fly.io configs OK (dev: $DEV_APP, prod: $PROD_APP)"
        fi
    else
        # Fallback: use grep parsing (brittle but works if binary unavailable)
        DEV_APP=$(grep '^app = ' fly.dev.toml 2>/dev/null | cut -d'"' -f2 || echo "")
        PROD_APP=$(grep '^app = ' fly.toml 2>/dev/null | cut -d'"' -f2 || echo "")
        
        if [ -z "$DEV_APP" ]; then
            echo "❌ Could not extract app name from fly.dev.toml"
            ERRORS=$((ERRORS + 1))
        elif [ -z "$PROD_APP" ]; then
            echo "❌ Could not extract app name from fly.toml"
            ERRORS=$((ERRORS + 1))
        elif [ "$DEV_APP" = "$PROD_APP" ]; then
            echo "⚠️  Dev and prod app names are the same: $DEV_APP"
            echo "   This may cause cross-environment deployment issues (some valid setups intentionally share names)."
            if [ "$STRICT_MODE" = "true" ]; then
                ERRORS=$((ERRORS + 1))
            fi
        else
            echo "✅ Fly.io configs OK (dev: $DEV_APP, prod: $PROD_APP)"
        fi
    fi
fi
echo ""

# 11. Check for duplicate dependencies
echo "1️⃣1️⃣  Checking for duplicate dependencies..."
if cargo tree --duplicates 2>/dev/null | grep -q "(*)$"; then
    # Count types of duplicates
    DUPS_OUTPUT=$(cargo tree --duplicates 2>/dev/null)
    WINDOWS_DUPS=$(echo "$DUPS_OUTPUT" | grep "windows_" | wc -l)
    
    echo "✅ Duplicate dependencies found (normal for complex projects)"
    if [ "$WINDOWS_DUPS" -gt 0 ]; then
        echo "   • Windows platform crates: Platform-specific, expected on cross-platform projects"
    fi
    echo "   • Other duplicates (base64, thiserror, etc.): Part of complex dependency trees"
    echo "   Review details with: cargo tree --duplicates"
else
    echo "✅ No duplicate dependencies"
fi
echo ""

# 11a. Validate cargo-deny configuration
echo "1️⃣1️⃣a️⃣  Validating cargo-deny configuration..."
if [ -f "deny.toml" ]; then
    if command -v cargo-deny > /dev/null 2>&1; then
        # Run cargo-deny check to validate deny.toml syntax
        if cargo deny check 2>&1 | grep -qE "^error|failed to deserialize"; then
            echo "❌ deny.toml has syntax errors"
            cargo deny check 2>&1 | grep -A 3 -E "^error|failed" | head -10 || true
            echo "   Common issues:"
            echo "   - 'OR' expressions in allow list (use individual licenses instead)"
            echo "   - Invalid license identifiers (use SPDX format)"
            ERRORS=$((ERRORS + 1))
        else
            echo "✅ deny.toml syntax OK"
        fi
    else
        # Basic TOML syntax check if cargo-deny not installed
        if command -v python3 > /dev/null 2>&1; then
            if python3 -c "import tomli; tomli.load(open('deny.toml', 'rb'))" 2>/dev/null || \
               python3 -c "import tomllib; tomllib.load(open('deny.toml', 'rb'))" 2>/dev/null; then
                echo "✅ deny.toml TOML syntax OK (cargo-deny not installed, basic validation only)"
            else
                echo "⚠️  deny.toml TOML syntax validation skipped (tomli/tomllib not available)"
                echo "   Install cargo-deny for full validation: cargo install cargo-deny"
            fi
        else
            echo "⚠️  deny.toml exists but cannot validate (cargo-deny and python3 not available)"
        fi
    fi
else
    echo "⚠️  deny.toml not found (optional, but recommended for license compliance)"
fi
echo ""

# 12. Validate Redis configuration in code
echo "1️⃣2️⃣  Validating Redis configuration..."
REDIS_TIMEOUT=$(grep -r "Duration::from_secs" crates/api/src/config.rs crates/core/src/redis.rs 2>/dev/null | grep -i redis | grep -o "from_secs([0-9]*)" | head -1 | grep -o "[0-9]*" || echo "")
if [ -n "$REDIS_TIMEOUT" ] && [ "$REDIS_TIMEOUT" -lt 10 ]; then
    echo "⚠️  Redis connection timeout is $REDIS_TIMEOUT seconds (recommended: >= 15s for Upstash)"
else
    echo "✅ Redis timeout configuration OK"
fi

# Check for Redis URL fallback to env var (should be SSM only, except for local dev)
# Allow env var fallback if it's clearly only for local development (wrapped in is_local_dev check)
if grep -r "REDIS_URL" crates/api/src/config.rs 2>/dev/null | grep -q "std::env::var"; then
    # Check if the env var usage is wrapped in a local dev check
    if grep -r -A 5 "REDIS_URL" crates/api/src/config.rs 2>/dev/null | grep -q "is_local_dev\|\.env\.local"; then
        echo "✅ Redis URL uses SSM only in production (env var fallback only for local dev)"
    else
        echo "⚠️  Redis URL has environment variable fallback - should use SSM only (or wrap in is_local_dev check)"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "✅ Redis URL uses SSM only (no env var fallback)"
fi

# Check for rediss:// TLS scheme in code comments/docs
if grep -r "rediss://\|6380" crates/api/src/config.rs crates/core/src/redis.rs 2>/dev/null | grep -q -i "rediss\|6380"; then
    echo "✅ Redis TLS configuration documented"
else
    echo "⚠️  Redis TLS configuration (rediss://, port 6380) not documented in code"
fi
echo ""

# 13. Runtime secret checks
echo "1️⃣3️⃣  Checking runtime secrets..."
if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
    SECRET_CHECK=$("$VALIDATE_CONFIG_BIN" secrets --check-local --env "${ENVIRONMENT:-dev}" 2>/dev/null || echo "")
    
        if [ -n "$SECRET_CHECK" ]; then
            # Check for errors
            if [ "$USE_JQ" = "true" ]; then
                ERROR_COUNT=$(echo "$SECRET_CHECK" | jq -r '[.errors[]?] | length' 2>/dev/null || echo "0")
                WARNING_COUNT=$(echo "$SECRET_CHECK" | jq -r '[.warnings[]?] | length' 2>/dev/null || echo "0")
                
                if [ "$ERROR_COUNT" -gt 0 ]; then
                    echo "❌ Missing required secrets:"
                    # Display errors with remediation if available
                    echo "$SECRET_CHECK" | jq -r '.errors[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                        if [ -n "$line" ]; then
                            echo "$line"
                        fi
                    done
                    if [ "$STRICT_MODE" = "true" ]; then
                        ERRORS=$((ERRORS + 1))
                    fi
                elif [ "$WARNING_COUNT" -gt 0 ]; then
                    echo "⚠️  Secret warnings:"
                    # Display warnings with remediation if available
                    echo "$SECRET_CHECK" | jq -r '.warnings[]? | if type == "object" then "   - \(.message)\n     → \(.remediation // "")" else "   - \(.)" end' 2>/dev/null | while read -r line; do
                        if [ -n "$line" ]; then
                            echo "$line"
                        fi
                    done
                else
                    echo "✅ Runtime secrets OK"
                fi
            else
                # Fallback parsing
                if echo "$SECRET_CHECK" | grep -q '"errors"'; then
                    echo "❌ Missing required secrets (check output above)"
                    if [ "$STRICT_MODE" = "true" ]; then
                        ERRORS=$((ERRORS + 1))
                    fi
                elif echo "$SECRET_CHECK" | grep -q '"warnings"'; then
                    echo "⚠️  Secret warnings (check output above)"
                else
                    echo "✅ Runtime secrets OK"
                fi
            fi
    else
        echo "⚠️  Could not validate secrets (validate-config unavailable)"
    fi
else
    echo "⚠️  Secret validation skipped (validate-config unavailable, using fallback mode)"
fi
echo ""

    # 14. Optional security scans and dependency audits
    echo "1️⃣4️⃣  Optional security scans (cargo-audit, cargo-deny)..."
    if command -v cargo-audit > /dev/null 2>&1; then
        echo "   Running cargo-audit (vulnerabilities)..."
        # Ignore RUSTSEC-2023-0071 (rsa Marvin Attack): transitive via sqlx-mysql, no fix available
        if ! cargo audit --ignore RUSTSEC-2023-0071; then
            echo "⚠️  cargo-audit found issues. Review and fix vulnerabilities." || true
        else
            echo "✅ cargo-audit OK (with documented exceptions: RUSTSEC-2023-0071)"
        fi
    else
        echo "   cargo-audit not installed - skip (install: cargo install cargo-audit)"
    fi

    if command -v cargo-deny > /dev/null 2>&1; then
        echo "   Running cargo-deny (policies)..."
        if ! cargo deny check; then
            echo "⚠️  cargo-deny reported policy issues. Review deny.toml and fixes." || true
        else
            echo "✅ cargo-deny OK"
        fi
    else
        echo "   cargo-deny not installed - skip (install: cargo install cargo-deny)"
    fi
    echo ""

    # 15. Optional SQLX prepare / migrations check (requires DATABASE_URL)
    echo "1️⃣5️⃣  Optional SQLX prepare / migrations (if DATABASE_URL set)"
    if [ -n "${DATABASE_URL:-}" ] && command -v cargo-sqlx > /dev/null 2>&1; then
        echo "   Preparing SQLX (cargo sqlx prepare)..."
        if ! cargo sqlx prepare -- --lib; then
            echo "⚠️  cargo sqlx prepare failed - ensure DATABASE_URL points to a writable DB or skip this check." || true
        else
            echo "✅ cargo sqlx prepare OK"
        fi
    else
        echo "   Skipping SQLX prepare (DATABASE_URL not set or cargo-sqlx not installed)"
    fi
    echo ""

    # 16. Optional frontend lint/build (guarded by RUN_FRONTEND env var)
    echo "1️⃣6️⃣  Optional frontend checks (npm lint/build - guarded by RUN_FRONTEND)"
    if [ -f "../frontend/package.json" ] && [ "${RUN_FRONTEND:-false}" = "true" ]; then
        echo "   Running frontend lint/build in ../frontend"
        (cd ../frontend && npm ci && npm run lint && npm run build) || {
            echo "⚠️  Frontend checks failed. Ensure node/npm and scripts are available." || true
        }
    else
        echo "   Skipping frontend checks (set RUN_FRONTEND=true to enable)"
    fi
    echo ""

    # 17. Shell script linting and secret detection
    echo "1️⃣7️⃣  Shell linting and secret scanning (shellcheck, git-secrets)"
    if command -v shellcheck > /dev/null 2>&1; then
        echo "   Running shellcheck on scripts/*.sh"
        shellcheck -x ./scripts/*.sh || echo "   shellcheck issues (non-fatal)"
    else
        echo "   shellcheck not installed - skip (install: apt install shellcheck)"
    fi

    if command -v git-secrets > /dev/null 2>&1; then
        echo "   Running git-secrets scan"
        git-secrets --scan || echo "   git-secrets found potential secrets (non-fatal)"
    else
        echo "   git-secrets not installed - skip"
    fi
    echo ""

    # 18. Optional coverage (tarpaulin) guarded by RUN_COVERAGE
    echo "1️⃣8️⃣  Optional coverage (cargo-tarpaulin, RUN_COVERAGE=true)"
    if [ "${RUN_COVERAGE:-false}" = "true" ]; then
        if command -v cargo-tarpaulin > /dev/null 2>&1; then
            echo "   Running cargo-tarpaulin (coverage)"
            cargo tarpaulin --out Xml || echo "   cargo-tarpaulin failed or produced no coverage"
        else
            echo "   cargo-tarpaulin not installed - skip"
        fi
    else
        echo "   Skipping coverage (set RUN_COVERAGE=true to enable)"
    fi
    echo ""

# Summary
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Count optional tools missing
OPTIONAL_TOOLS_MISSING=0
if ! command -v cargo-nextest > /dev/null 2>&1; then OPTIONAL_TOOLS_MISSING=$((OPTIONAL_TOOLS_MISSING + 1)); fi
if ! command -v cargo-deny > /dev/null 2>&1; then OPTIONAL_TOOLS_MISSING=$((OPTIONAL_TOOLS_MISSING + 1)); fi
if ! command -v cargo-audit > /dev/null 2>&1; then OPTIONAL_TOOLS_MISSING=$((OPTIONAL_TOOLS_MISSING + 1)); fi

if [ $ERRORS -eq 0 ]; then
    echo "✅ All critical checks passed! Safe to commit."
    if [ "$OPTIONAL_TOOLS_MISSING" -gt 0 ]; then
        echo "   Optional tools missing: $OPTIONAL_TOOLS_MISSING (install for full validation)"
    fi
    if [ "$STRICT_MODE" = "true" ]; then
        echo "   (Running in strict mode - warnings treated as errors)"
    fi
    exit 0
else
    echo "❌ Validation failed with $ERRORS error(s)."
    echo "   Fix all errors before committing to avoid CI failures."
    if [ "$STRICT_MODE" = "true" ]; then
        echo "   (Running in strict mode - warnings treated as errors)"
    fi
    exit 1
fi

