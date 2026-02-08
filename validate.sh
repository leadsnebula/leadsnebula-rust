#!/bin/bash
# Pre-commit validation script
# Run this before every commit to ensure CI will pass
#
# Usage:
#   ./validate.sh          # Validation checks (formatting, linting, unit tests, configs)
#
# Environment variables (for WSL stability):
#   SKIP_CLEANUP=true      # Skip incremental artifact cleanup (NOT recommended - corrupt artifacts cause crashes)
#   SKIP_RELEASE_BUILD=true # Skip resource-intensive release build (recommended in WSL)
#   SKIP_CLIPPY=true       # Skip clippy check (not recommended - validate.sh should catch CI issues)
#   SKIP_TESTS=true        # Skip unit tests (only if needed)
#   SKIP_BUILD=true        # Skip release build (same as SKIP_RELEASE_BUILD)
#   ULTRA_SAFE_MODE=true   # Ultra-safe mode: skip all heavy operations (clippy, tests, build)
#                          # Only runs: formatting, module checks, config validation
#
# This script focuses on CI-style validation checks:
#   - Code formatting and linting
#   - Unit tests (no database required)
#   - Config validation (Cargo.toml, Dockerfile, workflows, Fly.io)
#   - Build verification
#
# For full test suite including database integration tests:
#   ./autotests.sh          # Creates ephemeral DB and runs all tests
#
# 💡 WSL Users: Script auto-detects WSL and applies safer defaults:
#    - SKIP_RELEASE_BUILD=true (skips resource-intensive release build)
#    - Cleanup runs by default (corrupt artifacts cause crashes - cleanup prevents them)
#    Override with: SKIP_RELEASE_BUILD=false ./validate.sh
#
# 🚨 Root Cause of WSL Crashes:
#    Corrupt incremental compilation artifacts (dep-graph.bin files) accumulate over time.
#    When clippy processes hundreds of corrupt files, WSL runs out of resources and crashes.
#    Solution: Cleanup removes these artifacts BEFORE running clippy.
#
# 🚨 If WSL still crashes after cleanup, use ultra-safe mode:
#    ULTRA_SAFE_MODE=true ./validate.sh
#    This skips all heavy operations and only runs lightweight checks

set -e

# Auto-detect WSL to apply safer defaults
if [ -f /proc/version ] && grep -qi microsoft /proc/version; then
    IS_WSL=true
else
    IS_WSL=false
fi

# Parse flags
STRICT_MODE=${CI:-false}  # Auto-enable in CI, default off locally
for arg in "$@"; do
    if [ "$arg" = "--strict" ]; then
        STRICT_MODE=true
    fi
done

# WSL-specific defaults: limit resources but still run clippy
# NOTE: Clippy can crash WSL when processing corrupt incremental artifacts or with high parallelism
# Cleanup runs by default to remove corrupt artifacts BEFORE clippy
# We limit CARGO_BUILD_JOBS for clippy in WSL to prevent memory exhaustion
if [ "$IS_WSL" = "true" ]; then
    # Clippy runs by default, but with limited parallelism to prevent crashes
    # User can still skip with SKIP_CLIPPY=true if needed
    if [ -z "${SKIP_CLIPPY:-}" ]; then
        # Clippy will run, but with CARGO_BUILD_JOBS=1 to prevent memory issues
        export CLIPPY_JOBS=1
        echo "🔧 WSL detected: Clippy will run with CARGO_BUILD_JOBS=1 to prevent crashes"
        echo "   Skip with: SKIP_CLIPPY=true ./validate.sh"
    fi
    if [ -z "${SKIP_RELEASE_BUILD:-}" ]; then
        export SKIP_RELEASE_BUILD=true
        echo "🔧 WSL detected: Auto-enabling SKIP_RELEASE_BUILD=true to prevent crashes"
        echo "   Override with: SKIP_RELEASE_BUILD=false ./validate.sh"
    fi
    echo ""
fi

# Ultra-safe mode for WSL: skip heavy operations that commonly cause crashes
# Set ULTRA_SAFE_MODE=true to skip: clippy, tests, build, and other heavy checks
# NOTE: We still run cleanup in ultra-safe mode - it's needed to prevent crashes
if [ "$IS_WSL" = "true" ] && [ "${ULTRA_SAFE_MODE:-false}" = "true" ]; then
    export SKIP_RELEASE_BUILD=true
    export SKIP_CLIPPY=true
    export SKIP_TESTS=true
    export SKIP_BUILD=true
    echo "🔧 WSL Ultra-Safe Mode: Skipping heavy operations (clippy, tests, build)"
    echo "   This mode only runs: cleanup, formatting, module checks, config validation"
    echo "   Disable with: ULTRA_SAFE_MODE=false ./validate.sh"
    echo ""
fi

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
    # Build with visible output (remove --quiet) so we can see what's happening
    # Redirect stderr to stdout so errors are visible
    # Add timeout to prevent WSL crashes (5 minutes for validate-config build)
    if ! timeout 300 cargo build --release --bin validate-config 2>&1 | tee /tmp/validate-config-build.log; then
        EXIT_CODE=${PIPESTATUS[0]}
        if [ $EXIT_CODE -eq 124 ]; then
            echo "⚠️  validate-config build timed out (5 minutes). This may indicate WSL/filesystem issues."
            echo "   Try: SKIP_CLEANUP=true ./validate.sh or restart WSL"
            echo "   Using fallback parsing"
        else
            echo "⚠️  validate-config build failed, using fallback parsing"
            echo "   Build output saved to /tmp/validate-config-build.log"
        fi
        USE_FALLBACK=true
    fi
fi

# If binary doesn't exist or build failed, use fallback
if [ ! -f "$VALIDATE_CONFIG_BIN" ]; then
    USE_FALLBACK=true
fi

echo "🔍 Running pre-commit validation checks..."
echo ""
echo "   This script validates code quality and runs unit tests (no database required)."
echo "   For full test suite including database integration tests, run: ./autotests.sh"
echo ""

ERRORS=0

# 0. Clean corrupt incremental artifacts (prevents WSL crashes)
# ROOT CAUSE: Corrupt incremental compilation artifacts (dep-graph.bin files) accumulate from WSL crashes
# and cause clippy/cargo to process hundreds of corrupt files, leading to resource exhaustion and crashes.
# Solution: Proactively clean corrupt files and empty directories before they accumulate
# This is safe because incremental compilation is a performance optimization, not required for correctness
# NOTE: We use targeted cleanup (individual files) first, then remove empty directories
# NOTE: This prevents accumulation of corrupt artifacts that cause WSL crashes
if [ "${SKIP_CLEANUP:-false}" != "true" ] && [ -d "target" ]; then
    echo "0️⃣  Cleaning corrupt incremental compilation artifacts (prevents WSL crashes)..."
    
    CLEANUP_SUCCESS=true
    CLEANED_FILES=0
    
    # Step 1: Clean corrupt dep-graph.bin files (symptom of WSL crashes)
    # These files are automatically regenerated by cargo, so safe to delete
    if [ -d "target/debug/incremental" ] || [ -d "target/release/incremental" ]; then
        echo "   Removing corrupt dep-graph.bin files..."
        # Use timeout and limit scope to prevent crashes
        (timeout 5 find target/debug/incremental target/release/incremental -name "dep-graph.bin" -type f -delete 2>/dev/null || true) || CLEANUP_SUCCESS=false
        # Count cleaned files (safe now that we've deleted the corrupt ones)
        CLEANED_FILES=$(find target/debug/incremental target/release/incremental -name "dep-graph.bin" -type f 2>/dev/null | wc -l || echo "0")
    fi
    
    # Step 2: Clean empty incremental directories (prevents stale artifacts)
    if [ -d "target/debug/incremental" ] || [ -d "target/release/incremental" ]; then
        echo "   Removing empty incremental directories..."
        (timeout 3 find target/debug/incremental target/release/incremental -type d -empty -delete 2>/dev/null || true) || CLEANUP_SUCCESS=false
    fi
    
    # Step 3: Clean potentially corrupted AWS SDK artifacts (can cause "Archive member size is too large" errors)
    if [ -d "target/debug/deps" ] || [ -d "target/release/deps" ]; then
        echo "   Removing potentially corrupted AWS SDK artifacts..."
        (timeout 5 find target/debug/deps target/release/deps -name "*aws_sdk_*" -type f -delete 2>/dev/null || true) || CLEANUP_SUCCESS=false
    fi
    
    # Step 4: If incremental directories are still problematic, remove them entirely
    # This is a last resort but safe (cargo regenerates them)
    if [ "$CLEANUP_SUCCESS" = "false" ] || [ "$CLEANED_FILES" -gt 10 ]; then
        echo "   Removing entire incremental directories (too many corrupt files)..."
        (timeout 3 rm -rf target/debug/incremental target/release/incremental 2>/dev/null || true) || true
    fi
    
    # Verify cleanup
    if [ ! -d "target/debug/incremental" ] && [ ! -d "target/release/incremental" ]; then
        echo "✅ Incremental directories removed (cargo will regenerate on next build)"
    elif [ "$CLEANUP_SUCCESS" = "true" ]; then
        echo "✅ Cleanup complete (corrupt files removed, incremental directories preserved)"
    else
        echo "⚠️  Cleanup may have failed - if WSL crashes, set SKIP_CLEANUP=true"
    fi
    echo ""
elif [ "${SKIP_CLEANUP:-false}" = "true" ]; then
    echo "0️⃣  Cleaning corrupt incremental compilation artifacts (SKIPPED - SKIP_CLEANUP=true)..."
    echo "   ⚠️  Cleanup skipped. If you experience WSL crashes, try running cleanup manually:"
    echo "   find target -name 'dep-graph.bin' -delete && find target -type d -empty -delete"
    echo ""
fi

# 1. Format check
echo "1️⃣  Checking formatting..."
# Add timeout to prevent WSL crashes from hanging cargo commands
# Use shorter timeout in WSL to fail fast
TIMEOUT_FMT=$([ "$IS_WSL" = "true" ] && echo "120" || echo "180")
if ! timeout "$TIMEOUT_FMT" cargo fmt --all -- --check; then
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo "❌ Formatting check timed out (3 minutes). This may indicate WSL/filesystem issues."
        echo "   Try: SKIP_CLEANUP=true ./validate.sh or restart WSL"
        ERRORS=$((ERRORS + 1))
    else
        echo "❌ Formatting check failed. Run 'cargo fmt --all' to fix."
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "✅ Formatting OK"
fi
echo ""

# 2. Clippy check
if [ "${SKIP_CLIPPY:-false}" = "true" ]; then
    echo "2️⃣  Running clippy (SKIPPED - SKIP_CLIPPY=true)..."
    echo "   ⚠️  Clippy skipped. This is recommended in WSL to prevent crashes."
    echo ""
else
    echo "2️⃣  Running clippy (warnings as errors)..."
    # Add timeout to prevent WSL crashes from hanging cargo commands
    # Use shorter timeout in WSL to fail fast
    # Set resource limits to prevent memory exhaustion
    TIMEOUT_CLIPPY=$([ "$IS_WSL" = "true" ] && echo "600" || echo "300")  # 10 min in WSL (sequential), 5 min elsewhere
    (ulimit -v 2097152 2>/dev/null || true)  # Limit virtual memory to 2GB for clippy
    
    # In WSL, limit parallelism for clippy to prevent memory exhaustion
    # Clippy compiles code, so it needs the same safeguards as regular compilation
    if [ "$IS_WSL" = "true" ] && [ -n "${CLIPPY_JOBS:-}" ]; then
        export CARGO_BUILD_JOBS="$CLIPPY_JOBS"
        echo "   Using CARGO_BUILD_JOBS=$CLIPPY_JOBS for clippy (WSL safety)"
    fi
    
    if ! timeout "$TIMEOUT_CLIPPY" cargo clippy --workspace --all-targets --all-features -- -D warnings; then
        EXIT_CODE=$?
        if [ $EXIT_CODE -eq 124 ]; then
            echo "❌ Clippy check timed out (5 minutes). This may indicate WSL/filesystem issues."
            echo "   Try: SKIP_CLIPPY=true ./validate.sh or restart WSL"
            ERRORS=$((ERRORS + 1))
        else
            echo "❌ Clippy check failed. Fix all warnings before committing."
            ERRORS=$((ERRORS + 1))
        fi
    else
        echo "✅ Clippy OK"
    fi
    echo ""
fi
# 3. Rust module references check - catch missing `mod` files early
echo "3️⃣  Checking Rust 'mod' references exist..."
MISSING_MODS=0
# For each .rs file under crates/, look for `mod name;` or `pub mod name;` declarations
# Use timeout and limit depth to prevent WSL crashes with large directory trees
if [ -d "crates" ]; then
    while IFS= read -r file; do
        # Skip if file doesn't exist or isn't readable (may have been deleted)
        [ -r "$file" ] || continue
        # For each matching line in the file
        while IFS= read -r line; do
            mod=$(echo "$line" | sed -E 's/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+([a-zA-Z0-9_]+);.*/\2/')
            [ -z "$mod" ] && continue
            dir=$(dirname "$file")
            if [ ! -f "$dir/$mod.rs" ] && [ ! -f "$dir/$mod/mod.rs" ]; then
                echo "❌ Missing module '$mod' referenced in $file"
                MISSING_MODS=1
            fi
        done < <(grep -E '^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[a-zA-Z0-9_]+;' "$file" 2>/dev/null || true)
    done < <(timeout 10 find crates -maxdepth 6 -type f \( -name 'mod.rs' -o -path '*/src/services/*.rs' -o -path '*/src/models/mod.rs' -o -path '*/src/routes/mod.rs' -o -path '*/tests/*.rs' \) -print 2>/dev/null || true)
fi

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
        # ROOT CAUSE: `cargo nextest list` compiles the entire workspace to list tests,
        # which can cause WSL resource exhaustion and crashes.
        # Solution: Skip in WSL, or use a simple TOML syntax check instead.
        if [ "$IS_WSL" = "true" ]; then
            echo "   ⚠️  Nextest validation skipped in WSL (cargo nextest list compiles workspace, can crash WSL)"
            echo "   ✅ Nextest config file exists (.config/nextest.toml)"
        else
            # Validate nextest config by trying to list tests (parses and validates config)
            # Use --workspace to ensure config is loaded, but don't actually run tests
            # Add timeout to prevent hangs
            TIMEOUT_NEXTEST=$([ "$IS_WSL" = "true" ] && echo "30" || echo "60")
            if ! timeout "$TIMEOUT_NEXTEST" cargo nextest list --workspace --locked > /dev/null 2>&1; then
                EXIT_CODE=$?
                if [ $EXIT_CODE -eq 124 ]; then
                    echo "⚠️  Nextest config validation timed out (may indicate resource issues)"
                else
                    echo "❌ Nextest config (.config/nextest.toml) is invalid"
                    echo "   Attempting to parse config to show error:"
                    timeout 10 cargo nextest list --workspace --locked 2>&1 | grep -A 5 "nextest\|config\|error" | head -10 || true
                fi
                if [ "$STRICT_MODE" = "true" ]; then
                    ERRORS=$((ERRORS + 1))
                else
                    echo "⚠️  Nextest config validation failed (non-blocking in non-strict mode)"
                fi
            else
                echo "✅ Nextest config OK"
            fi
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

# 4. Unit tests only (no database required)
if [ "${SKIP_TESTS:-false}" = "true" ]; then
    echo "4️⃣  Running unit tests (SKIPPED - SKIP_TESTS=true)..."
    echo "   ⚠️  Tests skipped. This is recommended in WSL to prevent crashes."
    echo "   Run tests manually with: cargo test --lib"
    echo ""
else
    echo "4️⃣  Running unit tests (with --locked)..."
    echo "   Note: This script runs unit tests only. For full test suite including database"
    echo "   integration tests, run: ./autotests.sh"
    echo ""
    echo "   ⚠️  Database tests may fail with:"
    echo "      - Migration table race conditions (_sqlx_migrations does not exist)"
    echo "      - PoolTimedOut errors (tests taking >60 seconds)"
    echo "      Run ./autotests.sh to catch these issues before pushing"
    echo ""

    if command -v cargo-nextest > /dev/null 2>&1; then
    echo "   Using cargo-nextest for faster parallel test execution..."
    
    # Build test binaries once upfront (matches CI optimization pattern)
    echo "   🔨 Building unit test binaries upfront (compile once, run many times)..."
    TIMEOUT_BUILD=$([ "$IS_WSL" = "true" ] && echo "300" || echo "180")
    if ! timeout "$TIMEOUT_BUILD" cargo nextest list --lib --locked --all-features > /dev/null 2>&1; then
        # Fallback: use cargo test --no-run if nextest list fails
        timeout "$TIMEOUT_BUILD" cargo test --no-run --lib --locked --all-features > /dev/null 2>&1 || true
    fi
    echo "   ✅ Test binaries built - running tests (no recompilation)..."
    
    # Add timeout to prevent WSL crashes (10 minutes for unit tests, shorter in WSL)
    TIMEOUT_TESTS=$([ "$IS_WSL" = "true" ] && echo "480" || echo "600")
    if ! timeout "$TIMEOUT_TESTS" cargo nextest run --lib --locked --all-features; then
        EXIT_CODE=$?
        if [ $EXIT_CODE -eq 124 ]; then
            echo "❌ Unit tests timed out (10 minutes). This may indicate WSL/filesystem issues."
            echo "   Try: SKIP_CLEANUP=true ./validate.sh or restart WSL"
            ERRORS=$((ERRORS + 1))
        else
            echo "❌ Unit tests failed. Fix all failing tests before committing."
            ERRORS=$((ERRORS + 1))
        fi
    else
        echo "✅ Unit tests OK"
    fi
else
        echo "   Using cargo test (install cargo-nextest for faster tests: cargo install cargo-nextest)"
        
        # Build test binaries once upfront (matches CI optimization pattern)
        echo "   🔨 Building unit test binaries upfront (compile once, run many times)..."
        TIMEOUT_BUILD=$([ "$IS_WSL" = "true" ] && echo "300" || echo "180")
        (ulimit -v 2097152 2>/dev/null || true)  # Limit virtual memory to 2GB for build
        timeout "$TIMEOUT_BUILD" cargo test --no-run --lib --locked --all-features > /dev/null 2>&1 || true
        echo "   ✅ Test binaries built - running tests (no recompilation)..."
        
        # Add timeout to prevent WSL crashes (10 minutes for unit tests, shorter in WSL)
        # Set resource limits to prevent memory exhaustion
        TIMEOUT_TESTS=$([ "$IS_WSL" = "true" ] && echo "480" || echo "600")
        (ulimit -v 2097152 2>/dev/null || true)  # Limit virtual memory to 2GB for tests
        if ! timeout "$TIMEOUT_TESTS" cargo test --lib --locked --all-features; then
            EXIT_CODE=$?
            if [ $EXIT_CODE -eq 124 ]; then
                echo "❌ Unit tests timed out (10 minutes). This may indicate WSL/filesystem issues."
                echo "   Try: SKIP_TESTS=true ./validate.sh or restart WSL"
                ERRORS=$((ERRORS + 1))
            else
                echo "❌ Unit tests failed. Fix all failing tests before committing."
                ERRORS=$((ERRORS + 1))
            fi
        else
            echo "✅ Unit tests OK"
        fi
    fi
    echo ""
    echo ""
fi

# 5. Build check
# SKIP_RELEASE_BUILD can be set to skip the resource-intensive release build
# This is useful in WSL where full release builds can cause crashes
if [ "${SKIP_BUILD:-false}" = "true" ] || [ "${SKIP_RELEASE_BUILD:-false}" = "true" ]; then
    echo "5️⃣  Build check (SKIPPED - SKIP_BUILD/SKIP_RELEASE_BUILD=true)..."
    echo "   ⚠️  Release build skipped. Use 'cargo build --release' manually to verify."
    echo "   (This is recommended in WSL to prevent resource exhaustion)"
    echo ""
else
    echo "5️⃣  Building release (with --locked, limited parallelism for WSL stability)..."
    # Limit parallelism to reduce memory/CPU usage and prevent WSL crashes
    # Use 1 job in WSL, 2 jobs elsewhere to be more conservative
    # Add timeout to prevent WSL crashes from hanging cargo commands
    # Set resource limits to prevent memory exhaustion
    BUILD_JOBS=$([ "$IS_WSL" = "true" ] && echo "1" || echo "2")
    TIMEOUT_BUILD=$([ "$IS_WSL" = "true" ] && echo "480" || echo "600")
    (ulimit -v 3145728 2>/dev/null || true)  # Limit virtual memory to 3GB for release build
    if ! timeout "$TIMEOUT_BUILD" sh -c "CARGO_BUILD_JOBS=$BUILD_JOBS cargo build --release --locked"; then
        EXIT_CODE=$?
        if [ $EXIT_CODE -eq 124 ]; then
            echo "❌ Build timed out (10 minutes). This may indicate WSL/filesystem issues."
            echo "   💡 Hint: Try SKIP_RELEASE_BUILD=true ./validate.sh to skip release build"
            echo "   💡 Hint: Or try SKIP_CLEANUP=true ./validate.sh or restart WSL"
            ERRORS=$((ERRORS + 1))
        else
            echo "❌ Build failed. Fix compilation errors before committing."
            ERRORS=$((ERRORS + 1))
        fi
    else
        echo "✅ Build OK"
    fi
    echo ""
fi

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

# Validate test helper configuration (check for proper timeout settings)
if [ -f "crates/core/src/test_helpers.rs" ]; then
    echo "3️⃣a️⃣  Validating test helper configuration..."
    if grep -q "acquire_timeout.*120.*CI\|120.*CI.*acquire_timeout" crates/core/src/test_helpers.rs && \
       grep -q "max_connections.*50.*CI\|50.*CI.*max_connections" crates/core/src/test_helpers.rs; then
        echo "✅ Test helpers have proper CI timeout settings (120s timeout, 50 max connections)"
    else
        echo "⚠️  Test helpers may not have optimal CI timeout settings"
        echo "   Expected: acquire_timeout=120s and max_connections=50 for CI"
        echo "   Check: crates/core/src/test_helpers.rs"
    fi
    echo ""
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
    # Add timeout to prevent WSL crashes
    if ! timeout 300 cargo check --workspace > /dev/null 2>&1; then
        EXIT_CODE=$?
        if [ $EXIT_CODE -eq 124 ]; then
            echo "❌ Cargo check timed out (5 minutes). This may indicate WSL/filesystem issues."
            echo "   Try: SKIP_CLEANUP=true ./validate.sh or restart WSL"
            CARGO_TOML_ERRORS=$((CARGO_TOML_ERRORS + 1))
        else
            echo "❌ Cargo.toml has issues. Check for version conflicts or missing dependencies."
            CARGO_TOML_ERRORS=$((CARGO_TOML_ERRORS + 1))
        fi
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
    # Try validate-config binary first, fall back to grep if it fails
    DEV_APP=""
    PROD_APP=""
    
    if [ "$USE_FALLBACK" = "false" ] && [ -f "$VALIDATE_CONFIG_BIN" ]; then
        # Use validate-config binary for reliable parsing
        DEV_OUTPUT=$("$VALIDATE_CONFIG_BIN" fly-toml fly.dev.toml 2>/dev/null || echo "")
        PROD_OUTPUT=$("$VALIDATE_CONFIG_BIN" fly-toml fly.toml 2>/dev/null || echo "")
        
        # Check if output is valid JSON (contains "app_name" field)
        if echo "$DEV_OUTPUT" | grep -q '"app_name"'; then
            DEV_APP=$(extract_json_field ".app_name" "$DEV_OUTPUT")
        fi
        if echo "$PROD_OUTPUT" | grep -q '"app_name"'; then
            PROD_APP=$(extract_json_field ".app_name" "$PROD_OUTPUT")
        fi
    fi
    
    # Fallback to grep if binary failed or unavailable
    if [ -z "$DEV_APP" ]; then
        DEV_APP=$(grep '^app = ' fly.dev.toml 2>/dev/null | cut -d'"' -f2 || echo "")
    fi
    if [ -z "$PROD_APP" ]; then
        PROD_APP=$(grep '^app = ' fly.toml 2>/dev/null | cut -d'"' -f2 || echo "")
    fi
    
    # Validate extracted app names
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
fi
echo ""

# 11. Check for duplicate dependencies
echo "1️⃣1️⃣  Checking for duplicate dependencies..."
# Add timeout to prevent WSL crashes
if timeout 60 cargo tree --duplicates 2>/dev/null | grep -q "(*)$"; then
    # Count types of duplicates
    DUPS_OUTPUT=$(timeout 60 cargo tree --duplicates 2>/dev/null)
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
        # Ignore RUSTSEC-2026-0009 (time DoS): transitive; patched in time>=0.3.47 when deps upgrade
        # Add timeout to prevent WSL crashes from hanging network operations
        # Use set +e temporarily to capture exit code without triggering set -e
        set +e
        AUDIT_OUTPUT=$(timeout 60 cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2026-0009 2>&1)
        AUDIT_EXIT=$?
        set -e
        if [ $AUDIT_EXIT -ne 0 ]; then
            # Check for read-only filesystem errors (common in WSL)
            if echo "$AUDIT_OUTPUT" | grep -qE "read-only|failed to obtain lock"; then
                echo "⚠️  cargo-audit skipped (read-only filesystem - common in WSL)"
                echo "   💡 Hint: This is expected in WSL with read-only cargo cache"
            elif [ $AUDIT_EXIT -eq 124 ]; then
                echo "⚠️  cargo-audit timed out (60s). This may indicate network/filesystem issues."
            else
                echo "⚠️  cargo-audit found issues. Review and fix vulnerabilities." || true
            fi
        else
            echo "✅ cargo-audit OK (with documented exceptions: RUSTSEC-2023-0071, RUSTSEC-2026-0009)"
        fi
    else
        echo "   cargo-audit not installed - skip (install: cargo install cargo-audit)"
    fi

    if command -v cargo-deny > /dev/null 2>&1; then
        echo "   Running cargo-deny (policies)..."
        # Add timeout to prevent WSL crashes from hanging operations
        # Use set +e temporarily to capture exit code without triggering set -e
        set +e
        DENY_OUTPUT=$(timeout 60 cargo deny check 2>&1)
        DENY_EXIT=$?
        set -e
        if [ $DENY_EXIT -ne 0 ]; then
            # Check for read-only filesystem errors (common in WSL)
            if echo "$DENY_OUTPUT" | grep -qE "read-only|failed to obtain lock|failed to acquire.*lock"; then
                echo "⚠️  cargo-deny skipped (read-only filesystem - common in WSL)"
                echo "   💡 Hint: This is expected in WSL with read-only cargo cache"
            elif [ $DENY_EXIT -eq 124 ]; then
                echo "⚠️  cargo-deny timed out (60s). This may indicate network/filesystem issues."
            else
                echo "⚠️  cargo-deny reported policy issues. Review deny.toml and fixes." || true
            fi
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
        # Add timeout to prevent WSL crashes from hanging database connections
        if ! timeout 30 cargo sqlx prepare -- --lib 2>&1; then
            echo "⚠️  cargo sqlx prepare failed or timed out - ensure DATABASE_URL points to a writable DB or skip this check." || true
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

    # 19. Optional Neon CLI validation (if NEON_API_KEY and NEON_PROJECT_ID are set)
    echo "1️⃣9️⃣  Optional Neon CLI validation (if NEON_API_KEY set)"
    if [ -f ".env.local" ]; then
        # Load NEON_API_KEY and NEON_PROJECT_ID from .env.local if not already set
        if [ -z "${NEON_API_KEY:-}" ]; then
            export $(grep "^NEON_API_KEY=" .env.local 2>/dev/null | xargs) 2>/dev/null || true
        fi
        if [ -z "${NEON_PROJECT_ID:-}" ]; then
            export $(grep "^NEON_PROJECT_ID=" .env.local 2>/dev/null | xargs) 2>/dev/null || true
        fi
    fi
    
    if [ -n "${NEON_API_KEY:-}" ] && [ -n "${NEON_PROJECT_ID:-}" ]; then
        echo "   Testing Neon CLI authentication and access..."
        export NEONCTL_API_KEY="$NEON_API_KEY"
        
        # Check if neonctl is available (via npx)
        if command -v npx > /dev/null 2>&1; then
            # Test neonctl version (with timeout to prevent WSL crashes from hanging network calls)
            if timeout 15 npx --yes neonctl --version > /dev/null 2>&1; then
                echo "   ✅ neonctl available via npx"
                
                # Test authentication by listing branches (with timeout to prevent WSL crashes)
                if timeout 20 npx --yes neonctl branches list --project "$NEON_PROJECT_ID" --output json > /dev/null 2>&1; then
                    echo "   ✅ Neon CLI authentication OK (can list branches)"
                else
                    echo "   ⚠️  Neon CLI authentication failed, timed out, or key lacks permissions"
                    echo "      (This may be OK if using a restricted org key - create/delete should still work)"
                fi
            else
                echo "   ⚠️  neonctl not available via npx or timed out (install: npm install -g neonctl or use npx)"
            fi
        else
            echo "   ⚠️  npx not available - cannot test neonctl (install: npm install -g npm)"
        fi
    else
        echo "   Skipping Neon CLI validation (NEON_API_KEY or NEON_PROJECT_ID not set)"
        echo "      Set in .env.local or environment to test Neon CLI before CI runs"
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

