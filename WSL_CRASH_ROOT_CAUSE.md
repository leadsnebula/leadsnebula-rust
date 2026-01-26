# WSL Crash Root Cause Analysis & Fix

## Problem
WSL was crashing with errors:
- `Error code: Wsl/Service/0x80072746` - "An existing connection was forcibly closed by the remote host"
- `Error code: Wsl/Service/CreateInstance/CreateVm/HCS/0x800705aa` - "Insufficient system resources exist to complete the requested service"

These crashes occurred:
1. **During test execution** (when pre-compiling all test targets)
2. **During Cursor server installation/reconnection** (when WSL was already under memory pressure from compilation)

## Root Cause Identified

### Issue #1: Pre-Compilation Step (FIXED)
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

### Issue #2: Unlimited CARGO_BUILD_JOBS in WSL (FIXED)
Even after removing pre-compilation, the script was setting:
```bash
export CARGO_BUILD_JOBS=$CPU_CORES  # 20 cores
```

This caused:
1. **All compilation during tests** to use 20 parallel jobs
2. **Memory exhaustion** when Cursor tried to reconnect/install server
3. **WSL crashes** because no headroom for system processes

### Why This Crashes WSL
- WSL has memory limits (typically 50% of host RAM, but can be lower)
- 20 parallel compilation jobs = 20-40GB+ memory usage
- Cursor server installation/reconnection needs additional memory
- WSL VM crashes when resources are exhausted
- Crashes happen during compilation OR when Cursor tries to reconnect

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

### Fix #1: Removed Pre-Compilation Step
The pre-compilation step has been **completely removed** from `autotestsall.sh`.

### Fix #2: Limited CARGO_BUILD_JOBS in WSL
Changed from:
```bash
export CARGO_BUILD_JOBS=$CPU_CORES  # 20 cores = 20-40GB memory
```

To:
```bash
# WSL: Use 60% of cores max (leaves 40% headroom for Cursor/system processes)
# Minimum 4, maximum 12 to prevent memory exhaustion
WSL_JOBS=$((CPU_CORES * 60 / 100))
if [ $WSL_JOBS -lt 4 ]; then WSL_JOBS=4; fi
if [ $WSL_JOBS -gt 12 ]; then WSL_JOBS=12; fi
export CARGO_BUILD_JOBS=$WSL_JOBS  # 12 cores max = ~12-24GB memory
```

### Why This Works
1. **Cargo's incremental compilation** handles compilation automatically
2. **Each test run compiles incrementally** - only what's needed
3. **No upfront resource exhaustion** - compilation happens gradually
4. **Limited parallel jobs** - leaves headroom for Cursor server and system processes
5. **Matches CI behavior** - CI uses `CARGO_BUILD_JOBS=4`, we use 4-12 in WSL
6. **Cached artifacts** from previous runs are reused automatically

### Execution Flow (After Fix)
1. Unit tests run → compiles unit test dependencies (incremental)
2. Integration tests run → compiles integration test dependencies (incremental)
3. Database tests run → compiles database test dependencies (incremental)
4. Each step reuses cached artifacts from previous steps

## Prevention

### What NOT to Do
- ❌ Don't pre-compile with `cargo test --no-run --all-features`
- ❌ Don't compile all test targets upfront
- ❌ Don't use unlimited parallel jobs (`CARGO_BUILD_JOBS=$CPU_CORES` in WSL)
- ❌ Don't exhaust all WSL memory (leave headroom for Cursor/system processes)

### What TO Do
- ✅ Let Cargo handle incremental compilation automatically
- ✅ Run tests directly (Cargo compiles as needed)
- ✅ Limit `CARGO_BUILD_JOBS` in WSL (use 60% of cores max, cap at 12)
- ✅ Leave memory headroom for Cursor server installation/reconnection
- ✅ Trust Cargo's caching system

## Verification

The script now:
1. ✅ Skips pre-compilation (removed)
2. ✅ Runs tests directly (matches CI)
3. ✅ Uses incremental compilation (automatic)
4. ✅ Limits `CARGO_BUILD_JOBS` in WSL (4-12 cores, prevents memory exhaustion)
5. ✅ Leaves memory headroom for Cursor server and system processes
6. ✅ Uses system linker to prevent linker OOM (already in place)

## Expected Behavior

- **No WSL crashes** during test execution
- **Faster overall execution** (no wasted pre-compilation time)
- **Gradual resource usage** (compilation happens incrementally)
- **Matches CI behavior** (predictable test results)

## Issue #3: Corrupted Build Artifacts (FIXED)

During WSL crashes, compilation processes can be killed mid-write, leaving corrupted `.rlib` files in `target/debug/deps`. These corrupted artifacts cause compilation errors on subsequent runs:

### Symptoms
- `error[E0786]: found invalid metadata files for crate 'proptest'`
- `failed to parse rlib: Archive member size is too large`
- Compilation fails even with low `CARGO_BUILD_JOBS` (corruption already exists)

### Root Cause
- WSL crashes during compilation → processes killed mid-write
- Partial `.rlib` files left behind → invalid archive format
- Rustc refuses to use corrupted artifacts → compilation aborts

### Solution
The `cleanup_artifacts()` function in `autotestsall.sh` now:
1. **Removes corrupted rlib files** from known problematic crates:
   - `proptest` (E0786 "Archive member size is too large")
   - `loom` ("truncated or malformed archive" linker errors)
   - `aws-sdk-ssm`, `aws-sdk-sesv2` ("memory map must have a non-zero length")
2. **Detects and removes ANY corrupted rlib files** by attempting to read them with `ar`/`llvm-ar`
3. **Cleans incremental compilation state** for affected crates
4. **Runs `cargo clean -p <crate>`** for each affected crate to ensure clean rebuild

### Prevention
- Clean artifacts before test runs (automatic in `autotestsall.sh`)
- Use `CARGO_BUILD_JOBS=2` in WSL to reduce crash probability
- Disable incremental compilation (`CARGO_INCREMENTAL=0`) for tests
- If corruption persists, run: `rm -rf target/debug/deps/libproptest-* target/debug/incremental`

## Files Changed

- `rust/autotestsall.sh`:
  - Removed pre-compilation step (lines 130-137)
  - Limited `CARGO_BUILD_JOBS` in WSL to 2 jobs (maximum safety)
  - Added cleanup for corrupted `proptest` artifacts (prevents E0786 errors)
