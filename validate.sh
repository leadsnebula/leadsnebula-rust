#!/bin/bash
# Pre-commit validation script
# Run this before every commit to ensure CI will pass

set -e

echo "🔍 Running pre-commit validation checks..."
echo ""

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

# 3. Test check
echo "3️⃣  Running tests (with --locked)..."
if ! cargo test --locked --all-features; then
    echo "❌ Tests failed. Fix all failing tests before committing."
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Tests OK"
fi
echo ""

# 4. Build check
echo "4️⃣  Building release (with --locked)..."
if ! cargo build --release --locked; then
    echo "❌ Build failed. Fix compilation errors before committing."
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Build OK"
fi
echo ""

# 5. Cargo.lock validation
echo "5️⃣  Validating Cargo.lock..."
if [ ! -f "Cargo.lock" ]; then
    echo "❌ Cargo.lock is missing. Run 'cargo generate-lockfile' and commit it."
    ERRORS=$((ERRORS + 1))
elif ! git ls-files --error-unmatch Cargo.lock > /dev/null 2>&1; then
    echo "❌ Cargo.lock is not tracked by git. Run 'git add Cargo.lock' and commit it."
    ERRORS=$((ERRORS + 1))
else
    # Check Cargo.lock version and verify Dockerfile Rust version compatibility
    LOCK_VERSION=$(grep "^version = " Cargo.lock | head -1 | cut -d' ' -f3)
    if [ "$LOCK_VERSION" = "4" ]; then
        # Cargo.lock v4 requires Rust 1.78+
        if [ -f "Dockerfile" ]; then
            DOCKER_RUST=$(grep "^FROM rust:" Dockerfile | head -1 | cut -d: -f2 | cut -d' ' -f1)
            if echo "$DOCKER_RUST" | grep -qE "^1\.(7[0-7]|[0-6][0-9])"; then
                echo "❌ Cargo.lock version 4 requires Rust 1.78+, but Dockerfile uses rust:$DOCKER_RUST"
                echo "   Update Dockerfile to use 'rust:bookworm' or 'rust:latest'"
                ERRORS=$((ERRORS + 1))
            else
                echo "✅ Cargo.lock version $LOCK_VERSION compatible with Dockerfile Rust version"
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

# 6. Cargo.toml validation
echo "6️⃣  Validating Cargo.toml..."
if ! cargo check --workspace > /dev/null 2>&1; then
    echo "❌ Cargo.toml has issues. Check for version conflicts or missing dependencies."
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Cargo.toml OK"
fi
echo ""

# 7. Docker validation (if Dockerfile exists)
if [ -f "Dockerfile" ]; then
    echo "7️⃣  Validating Dockerfile..."
    if command -v docker > /dev/null 2>&1; then
        # Check if Docker daemon is running
        if ! docker info > /dev/null 2>&1; then
            echo "⚠️  Docker daemon not running, skipping Docker build validation"
        else
            # Try to build the Docker image (just validation, don't push)
            echo "   Building Docker image for validation (this may take a while)..."
            if docker buildx build --load --platform linux/amd64 -t leadsnebula-rust:validate . > /tmp/docker-build.log 2>&1; then
                echo "✅ Dockerfile builds successfully"
                # Clean up test image
                docker rmi leadsnebula-rust:validate > /dev/null 2>&1 || true
            else
                echo "❌ Docker build failed. Check /tmp/docker-build.log for details."
                echo "   Common issues:"
                echo "   - Rust version mismatch (Cargo.lock version 4 requires Rust 1.78+)"
                echo "   - Missing files in COPY commands"
                echo "   - Cargo.lock not included in build context"
                tail -20 /tmp/docker-build.log
                ERRORS=$((ERRORS + 1))
            fi
        fi
    else
        echo "⚠️  Docker not available, skipping Dockerfile validation"
    fi
    echo ""
fi

# 8. Fly.io config validation
echo "8️⃣  Validating Fly.io configs..."
if [ ! -f "fly.toml" ]; then
    echo "⚠️  fly.toml not found (may be OK for dev-only setup)"
elif [ ! -f "fly.dev.toml" ]; then
    echo "⚠️  fly.dev.toml not found (may be OK for prod-only setup)"
else
    DEV_APP=$(grep '^app = ' fly.dev.toml 2>/dev/null | cut -d'"' -f2 || echo "")
    PROD_APP=$(grep '^app = ' fly.toml 2>/dev/null | cut -d'"' -f2 || echo "")
    
    if [ -z "$DEV_APP" ]; then
        echo "❌ Could not extract app name from fly.dev.toml"
        ERRORS=$((ERRORS + 1))
    elif [ -z "$PROD_APP" ]; then
        echo "❌ Could not extract app name from fly.toml"
        ERRORS=$((ERRORS + 1))
    elif [ "$DEV_APP" = "$PROD_APP" ]; then
        echo "❌ Dev and prod app names are the same: $DEV_APP"
        echo "   This will cause cross-environment deployment issues."
        ERRORS=$((ERRORS + 1))
    else
        echo "✅ Fly.io configs OK (dev: $DEV_APP, prod: $PROD_APP)"
    fi
fi
echo ""

# 9. Check for duplicate dependencies
echo "9️⃣  Checking for duplicate dependencies..."
if cargo tree --duplicates 2>/dev/null | grep -q "(*)$"; then
    echo "⚠️  Duplicate dependencies found. Review with 'cargo tree --duplicates'"
else
    echo "✅ No duplicate dependencies"
fi
echo ""

# Summary
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ $ERRORS -eq 0 ]; then
    echo "✅ All validation checks passed! Safe to commit."
    exit 0
else
    echo "❌ Validation failed with $ERRORS error(s)."
    echo "   Fix all errors before committing to avoid CI failures."
    exit 1
fi

