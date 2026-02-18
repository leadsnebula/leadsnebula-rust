# Cleanup Safety Measures

This document describes the cleanup safety measures implemented to prevent accidental deletion of source files.

## Problem

In commit `244520e`, a cleanup operation accidentally deleted critical source files:
- `crates/api/src/routes/carina.rs` (2124 lines of working code)
- `crates/api/tests/integration_carina_e2e.rs` (test file)
- `.github/workflows/rust-ci.yml` (CI/CD configuration)
- `.github/actions/setup-sccache/action.yml` (CI/CD action)

These files have been restored, and safeguards have been implemented to prevent future accidents.

## Implemented Safeguards

### 1. Safe Cleanup Scripts

**`scripts/clean-build-artifacts.sh`**
- Safely cleans only build artifacts in `target/` directory
- **Never deletes source files** (*.rs, *.toml, *.sql, *.sh, *.yml, *.yaml)
- Shows before/after size
- Multiple modes: normal, release-only, aggressive

**`scripts/check-target-size.sh`**
- Monitors `target/` directory size
- Warns if exceeds threshold (default: 50GB)
- Suggests cleanup when needed

### 2. Pre-Commit Protection

**`scripts/pre-commit-check-deletions.sh`**
- Git pre-commit hook to prevent deleting protected files
- Blocks commits that delete: `*.rs`, `*.toml`, `*.sql`, `*.sh`, `*.yml`, `*.yaml`
- Shows which files are being deleted
- Can be bypassed with `git commit --no-verify` (use carefully!)

**Installation:**
```bash
ln -s ../../scripts/pre-commit-check-deletions.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

### 3. Review Scripts

**`scripts/review-deletions.sh`**
- Review deletions before committing
- Shows deleted files grouped by type
- Highlights protected source files
- Can review staged, all, or specific commit deletions

**Usage:**
```bash
./scripts/review-deletions.sh              # Review staged deletions
./scripts/review-deletions.sh --all        # Review all deletions
./scripts/review-deletions.sh --commit <hash>  # Review specific commit
```

### 4. CI/CD Cleanup

Added automatic cleanup to CI/CD pipeline:
- Cleans build artifacts after each job (prevents accumulation)
- Runs even if previous steps failed (`if: always()`)
- Never touches source files

## Protected File Types

These file types are **never** deleted by cleanup scripts:

- `*.rs` - Rust source files
- `*.toml` - Cargo configuration files
- `*.sql` - Database migrations
- `*.sh` - Shell scripts
- `*.yml`, `*.yaml` - Configuration files (CI/CD, Docker, etc.)

## Restored Files

All deleted source files have been restored:

1. ✅ **`crates/api/src/routes/carina.rs`** → Restored as `crates/api/src/routes/leads.rs`
   - Full 2124-line implementation restored
   - Updated to use `leads_routes()` instead of `carina_routes()`
   - Compiles and works correctly

2. ✅ **`crates/api/tests/integration_carina_e2e.rs`**
   - Full test file restored
   - Compiles correctly
   - Tests are available via `autotestsall.sh`

3. ✅ **`.github/workflows/rust-ci.yml`**
   - CI/CD workflow restored
   - Includes cleanup steps

4. ✅ **`.github/actions/setup-sccache/action.yml`**
   - CI/CD action restored

## Best Practices

### Before Committing

1. **Always review deletions:**
   ```bash
   ./scripts/review-deletions.sh
   ```

2. **Check target size:**
   ```bash
   ./scripts/check-target-size.sh
   ```

3. **Clean if needed (safely):**
   ```bash
   ./scripts/clean-build-artifacts.sh
   ```

### Regular Maintenance

- Run `check-target-size.sh` weekly
- Run `clean-build-artifacts.sh` when `target/` exceeds 50GB
- Review deletions before large commits

### What to Clean

✅ **Safe to clean:**
- `target/` directory (build artifacts)
- Compiled binaries
- Incremental compilation cache
- Test artifacts

❌ **Never clean:**
- Source files (*.rs, *.toml, *.sql, *.sh, *.yml, *.yaml)
- Migrations
- Configuration files
- Scripts

## Verification

To verify the safeguards work:

```bash
# Test pre-commit hook
./scripts/pre-commit-check-deletions.sh

# Review a problematic commit
./scripts/review-deletions.sh --commit 244520e

# Check target size
./scripts/check-target-size.sh

# Clean safely
./scripts/clean-build-artifacts.sh
```

## Summary

- ✅ Safe cleanup scripts created
- ✅ Pre-commit protection implemented
- ✅ Review scripts available
- ✅ CI/CD cleanup automated
- ✅ All deleted source files restored
- ✅ Documentation created

The cleanup process is now **safe** and **protected** against accidental source file deletion.
