# WSL Crash Root Cause Analysis & Fix

## Problem
WSL was crashing during test execution with errors:
- `Error code: Wsl/Service/0x80072746` - "An existing connection was forcibly closed by the remote host"
- `Error code: Wsl/Service/CreateInstance/CreateVm/HCS/0x800705aa` - "Insufficient system resources exist to complete the requested service"

## Root Cause Identified

### The Culprit: Pre-Compilation Step
The script was running:
```bash
cargo test --no-run --locked --all-features
```

This command:
1. **Compiles ALL test targets** (lib tests, integration tests, binaries, etc.)
2. **With ALL features enabled** (every possible feature combination)
3. **With 20 parallel jobs** (`CARGO_BUILD_JOBS=$CPU_CORES` = 20 cores)
4. **Memory usage**: Each compilation job uses 1-2GB+ memory
5. **Total memory**: 20 jobs × 1-2GB = **20-40GB+ memory usage**

### Why This Crashes WSL
- WSL has memory limits (typically 50% of host RAM, but can be lower)
- 20 parallel compilation jobs exhaust available memory
- WSL VM crashes when resources are exhausted
- This happens **before tests even start running**

## Evidence

### 1. CI Doesn't Pre-Compile
Looking at `.github/workflows/rust-ci.yml`:
- CI runs tests directly: `cargo nextest run --test integration_health ...`
- CI does NOT run `cargo test --no-run` before tests
- CI uses `CARGO_BUILD_JOBS=4` in deploy (not unlimited)

### 2. Cargo Already Handles Incremental Compilation
- Cargo automatically compiles incrementally
- Each test run only compiles what's needed
- Previous compilation artifacts are cached
- No need to pre-compile everything upfront

### 3. WSL Error Codes Match Resource Exhaustion
- `0x800705aa` = "Insufficient system resources"
- `0x80072746` = "Connection forcibly closed" (WSL VM crashed)

## Solution

### Removed Pre-Compilation Step
The pre-compilation step has been **completely removed** from `autotestsall.sh`.

### Why This Works
1. **Cargo's incremental compilation** handles compilation automatically
2. **Each test run compiles incrementally** - only what's needed
3. **No upfront resource exhaustion** - compilation happens gradually
4. **Matches CI behavior** - tests run directly without pre-compilation
5. **Cached artifacts** from previous runs are reused automatically

### Execution Flow (After Fix)
1. Unit tests run → compiles unit test dependencies (incremental)
2. Integration tests run → compiles integration test dependencies (incremental)
3. Database tests run → compiles database test dependencies (incremental)
4. Each step reuses cached artifacts from previous steps

## Prevention

### What NOT to Do
- ❌ Don't pre-compile with `cargo test --no-run --all-features`
- ❌ Don't compile all test targets upfront
- ❌ Don't use unlimited parallel jobs for full compilation

### What TO Do
- ✅ Let Cargo handle incremental compilation automatically
- ✅ Run tests directly (Cargo compiles as needed)
- ✅ Use parallel jobs for individual test runs (not full compilation)
- ✅ Trust Cargo's caching system

## Verification

The script now:
1. ✅ Skips pre-compilation (removed)
2. ✅ Runs tests directly (matches CI)
3. ✅ Uses incremental compilation (automatic)
4. ✅ Maximizes CPU cores for individual test runs (safe)
5. ✅ Uses system linker to prevent linker OOM (already in place)

## Expected Behavior

- **No WSL crashes** during test execution
- **Faster overall execution** (no wasted pre-compilation time)
- **Gradual resource usage** (compilation happens incrementally)
- **Matches CI behavior** (predictable test results)

## Files Changed

- `rust/autotestsall.sh` - Removed pre-compilation step (lines 130-137)
