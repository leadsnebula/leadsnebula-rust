# Rust Project Scripts

This directory contains utility scripts for managing the Rust project.

## Build Artifact Management

### `clean-build-artifacts.sh`

Safely cleans Rust build artifacts from the `target/` directory. **Never deletes source files**.

**Usage:**
```bash
./scripts/clean-build-artifacts.sh          # Clean all build artifacts
./scripts/clean-build-artifacts.sh --release # Clean only release builds
./scripts/clean-build-artifacts.sh --aggressive  # Also clean cargo registry cache
```

**What it does:**
- Removes compiled binaries and incremental compilation artifacts
- Keeps downloaded dependencies (unless `--aggressive` is used)
- Shows before/after size
- **Never touches source files** (*.rs, *.toml, *.sql, *.sh, *.yml, *.yaml)

**When to use:**
- When `target/` directory grows too large (>50GB)
- Before committing to reduce repository size (though `target/` is gitignored)
- To free up disk space

### `check-target-size.sh`

Monitors `target/` directory size and warns if it exceeds a threshold.

**Usage:**
```bash
./scripts/check-target-size.sh          # Check and warn if > 50GB (default)
./scripts/check-target-size.sh 30        # Custom threshold in GB
```

**What it does:**
- Reports current `target/` size
- Warns if size exceeds threshold (default: 50GB)
- Suggests running `clean-build-artifacts.sh` if too large

**When to use:**
- As part of regular maintenance
- Before running large builds
- To monitor disk usage

## Deletion Safety

### `pre-commit-check-deletions.sh`

Pre-commit hook to prevent accidental deletion of source files.

**Usage:**
```bash
# Manual check
./scripts/pre-commit-check-deletions.sh

# Install as git pre-commit hook
ln -s ../../scripts/pre-commit-check-deletions.sh .git/hooks/pre-commit
```

**What it does:**
- Checks staged deletions for protected file types
- Blocks commits that delete: `*.rs`, `*.toml`, `*.sql`, `*.sh`, `*.yml`, `*.yaml`
- Shows which files are being deleted
- Allows bypass with `git commit --no-verify` (use carefully!)

**Protected file types:**
- `*.rs` - Rust source files
- `*.toml` - Cargo configuration
- `*.sql` - Database migrations
- `*.sh` - Shell scripts
- `*.yml`, `*.yaml` - Configuration files

### `review-deletions.sh`

Review deletions before committing to catch accidental deletions.

**Usage:**
```bash
./scripts/review-deletions.sh              # Review staged deletions
./scripts/review-deletions.sh --all        # Review all deletions (staged + unstaged)
./scripts/review-deletions.sh --commit <hash>  # Review deletions in a specific commit
```

**What it does:**
- Lists all deleted files
- Groups by file type
- Highlights protected source files
- Shows diff summary
- Prompts for confirmation if protected files are deleted

**When to use:**
- Before committing large changes
- After running cleanup scripts
- To audit what was deleted in a commit
- To verify deletions are intentional

## Best Practices

### Before Committing

1. **Review deletions:**
   ```bash
   ./scripts/review-deletions.sh
   ```

2. **Check target size:**
   ```bash
   ./scripts/check-target-size.sh
   ```

3. **Clean if needed:**
   ```bash
   ./scripts/clean-build-artifacts.sh
   ```

### Regular Maintenance

Run these periodically to keep the project healthy:

```bash
# Check target size (weekly)
./scripts/check-target-size.sh

# Clean if > 50GB (monthly or as needed)
./scripts/clean-build-artifacts.sh
```

### CI/CD

The CI/CD pipeline automatically:
- Cleans build artifacts after each job (prevents accumulation)
- Uses cache mounts for faster builds
- Never deletes source files

## Troubleshooting

### "target/ directory is very large"

Run:
```bash
./scripts/clean-build-artifacts.sh
```

This will free up space by removing compiled artifacts. The next build will be slower (first time), but subsequent builds will be fast.

### "Protected files being deleted" error

If you see this error when committing:
1. Review the deletions: `./scripts/review-deletions.sh`
2. Verify deletions are intentional
3. If truly needed, use `git commit --no-verify` (but be careful!)

### Pre-commit hook not working

If the pre-commit hook isn't running:
1. Check if it's installed: `ls -la .git/hooks/pre-commit`
2. Install it: `ln -s ../../scripts/pre-commit-check-deletions.sh .git/hooks/pre-commit`
3. Make it executable: `chmod +x .git/hooks/pre-commit`

## Safety Guarantees

These scripts are designed to be **safe**:

✅ **Will never delete:**
- Source files (*.rs, *.toml, *.sql, *.sh, *.yml, *.yaml)
- Migrations
- Configuration files
- Scripts

✅ **Will only clean:**
- Build artifacts in `target/`
- Compiled binaries
- Incremental compilation cache
- Test artifacts

✅ **Always shows:**
- What will be deleted/cleaned
- Size before/after
- Warnings for protected files
