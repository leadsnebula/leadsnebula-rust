# CI/CD Bottleneck Analysis

## Executive Summary

The CI/CD pipeline is taking **30+ minutes** primarily due to:
1. **sccache compilation** (~4 minutes) - Installing from source every run
2. **Heavy integration tests** (~5-6 minutes) - Several tests taking 1-3 minutes each
3. **Sequential test execution** - Tests running with `--test-threads=1`
4. **Ineffective sccache usage** - Cache restored but no evidence of significant benefit

## Detailed Bottleneck Analysis

### 1. sccache Installation (4+ minutes per job)

**Problem:**
- `cargo install sccache --locked` compiles sccache from source every time
- Takes ~4 minutes 16 seconds to compile (see build.txt line 862-863)
- The sccache binary itself is cached, but compilation happens on every run

**Evidence:**
```
2026-01-19T01:10:50.9038524Z    Installing sccache v0.13.0
2026-01-19T01:15:06.6477734Z    Finished `release` profile [optimized] target(s) in 4m 16s
```

**Impact:** 
- Affects ALL jobs: lint, test-unit, test-integration, build, coverage
- Total waste: ~4 minutes × 5 jobs = 20 minutes per pipeline run

**Recommendation:**
- Use pre-built sccache binary from GitHub releases instead of compiling
- Or cache the compiled sccache binary more effectively
- Or remove sccache if cache hit rate is low (see section 4)

### 2. Heavy Integration Tests (5-6 minutes)

**Problem:**
Several integration tests are taking 1-3 minutes each:

| Test | Duration | Location |
|------|----------|----------|
| `test_otp_backup_codes_storage` | ~3 min | integration_auth.rs |
| `test_otp_enable_and_disable` | ~2 min | integration_auth.rs |
| `test_otp_setup_creates_secret` | ~1.5 min | integration_auth.rs |
| `test_passkey_credential_storage` | ~1.5 min | integration_auth.rs |
| `test_passkey_max_limit_enforcement` | ~1.5 min | integration_auth.rs |
| `test_password_hashing_and_verification` | ~2 min | integration_auth.rs |
| `test_user_password_verification` | ~2 min | integration_auth.rs |
| `test_user_status_affects_authentication` | ~1.5 min | integration_auth.rs |
| `test_deleted_publisher_email_reuse` | ~2 min | integration_publisher_crud.rs |

**Total:** ~133 seconds for auth tests + ~128 seconds for publisher tests = **~4.5 minutes**

**Evidence:**
```
2026-01-20T20:07:56.3408998Z running 12 tests
2026-01-20T20:10:10.2375234Z test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 133.90s
```

**Recommendation:**
- Investigate why these tests are slow (likely database operations, password hashing, or network calls)
- Consider parallelizing tests (currently `--test-threads=1`)
- Optimize slow database operations
- Consider splitting heavy tests into separate job or running them less frequently

### 3. Sequential Test Execution

**Problem:**
- Integration tests use `--test-threads=1` (line 241-247 in rust-ci.yml)
- Forces all tests to run sequentially
- Unit tests use `cargo nextest` (parallel), but integration tests don't

**Current:**
```yaml
cargo test --test integration_health --locked --all-features -- --test-threads=1
cargo test --test integration_auth --locked --all-features -- --test-threads=1
```

**Recommendation:**
- Use `cargo nextest` for integration tests too
- Or increase `--test-threads` to 2-4 (balance between speed and database contention)
- Consider test isolation improvements to allow more parallelism

### 4. sccache Effectiveness Question

**Problem:**
- sccache takes 4+ minutes to install
- Cache is being restored (1GB cache), but no statistics showing cache hit rate
- No evidence in logs that sccache is providing significant benefit

**Evidence:**
- Cache restored: `Cache Size: ~1042 MB (1092134006 B)`
- But no `sccache --show-stats` output showing cache hits/misses
- Build still takes 8+ minutes even with sccache

**Recommendation:**
- Add `sccache --show-stats` after builds to measure effectiveness
- If cache hit rate < 50%, consider removing sccache
- The 4-minute install cost may exceed the benefit

### 5. cargo-nextest Installation

**Problem:**
- `cargo install cargo-nextest --locked` runs in test-integration job
- Unit tests job has better caching, but integration tests don't

**Current:**
- Unit tests: Has caching for cargo-nextest (line 282-296)
- Integration tests: No caching, installs every time

**Recommendation:**
- Add same caching strategy to integration tests job
- Or use a pre-built binary

### 6. neonctl via npx (14 seconds)

**Problem:**
- `npx --yes neonctl` downloads and runs every time
- Takes ~14 seconds per invocation

**Evidence:**
```
2026-01-20T20:07:11.0261481Z Setting up Neon CLI via npx...
2026-01-20T20:07:25.7113212Z 2.20.1
```

**Recommendation:**
- Cache npm cache or use a GitHub action that provides neonctl
- Or install globally once and cache it

### 7. Docker Build Dependencies

**Analysis:**
The Dockerfile only installs necessary dependencies:
- `build-essential` - Required for C compilation
- `pkg-config` - Required for finding libraries
- `libssl-dev` - Required for OpenSSL (used by many Rust crates)

**Verdict:** ✅ All dependencies are necessary and properly cached via Docker layer caching.

## Recommendations Priority

### High Priority (Immediate Impact)

1. **Remove or optimize sccache** (Saves ~4 min × 5 jobs = 20 min)
   - Option A: Use pre-built binary from releases
   - Option B: Remove if cache hit rate is low
   - Option C: Add statistics to measure effectiveness

2. **Parallelize integration tests** (Saves ~2-3 min)
   - Use `cargo nextest` or increase `--test-threads`
   - Ensure test isolation allows parallelism

3. **Cache cargo-nextest in integration tests** (Saves ~30 sec)

### Medium Priority

4. **Optimize slow integration tests** (Saves ~2-3 min)
   - Investigate why OTP/passkey/password tests are slow
   - Consider database query optimization
   - Consider test data setup optimization

5. **Cache neonctl** (Saves ~14 sec × 2 = 28 sec)

### Low Priority

6. **Review test coverage** - Some tests might be redundant or could be moved to unit tests

## Implementation Plan

### Phase 1: Quick Wins (Estimated 20-25 min savings)

1. Remove sccache or use pre-built binary
2. Add cargo-nextest caching to integration tests
3. Increase integration test parallelism

### Phase 2: Test Optimization (Estimated 2-3 min savings)

1. Profile slow integration tests
2. Optimize database operations
3. Improve test data setup

### Phase 3: Tool Optimization (Estimated 30 sec savings)

1. Cache neonctl installation
2. Review all cargo install commands

## Metrics to Track

After implementing changes, track:
- Total pipeline duration
- sccache cache hit rate (if keeping sccache)
- Individual test durations
- Cache hit rates for all cached artifacts
