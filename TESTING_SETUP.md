# Testing Setup and Infrastructure

This document describes the testing infrastructure and how to run tests.

## Test Structure

### Unit Tests (46+ tests)
- **Location**: Inline in source files with `#[cfg(test)]` modules
- **Coverage**: 
  - Encryption/decryption (with proptest)
  - JWT encoding/decoding
  - Password hashing/verification
  - Password policy validation
  - HMAC generation/verification
  - Ping tree winner selection algorithm
  - Cache service
  - Redis operations

### Integration Tests

#### Database Integration Tests
- **Location**: `tests/integration_publisher_crud.rs`
- **Requirements**: 
  - `DATABASE_URL` environment variable set (see setup below)
  - Test database with migrations applied
- **Tests**:
  - Publisher CRUD operations
  - Email uniqueness validation
  - Deleted publisher email reuse
- **Setup for Local Development**:
  
  **Option 1 (Recommended)**: Add to `.env.local`:
  ```bash
  DATABASE_URL=postgresql://user:password@localhost:5432/test_db
  ```
  
  The tests will automatically load `.env.local` if it exists. Environment variables take precedence.
  
  **Option 2**: Export in your shell:
  ```bash
  export DATABASE_URL="postgresql://user:password@localhost:5432/test_db"
  cargo test --test integration_publisher_crud
  ```
  
- **Run**: `cargo test --test integration_publisher_crud`
  
  **Note**: These tests require a running PostgreSQL database. The tests will automatically apply migrations.

#### API Route Integration Tests
- **Location**: `crates/api/tests/integration_routes.rs`
- **Tests**:
  - JWT service integration
  - Token expiration validation
- **Run**: `cargo test --test integration_routes`

### Concurrency Tests (Loom)
- **Location**: `crates/core/src/services/ping_tree_router_loom_tests.rs`
- **Purpose**: Test concurrent ping auction operations
- **Run**: `cargo test --lib services::ping_tree_router_loom_tests`

## Running Tests

### All Tests
```bash
cargo test --all-features
```

### Unit Tests Only
```bash
cargo test --lib
```

### Integration Tests
```bash
# Set DATABASE_URL first
export DATABASE_URL="postgresql://user:pass@localhost/dbname"
cargo test --test integration_publisher_crud
```

### With Nextest (Faster)
```bash
cargo nextest run --all-features
```

## Code Coverage

### Generate Coverage Report
```bash
./scripts/coverage.sh
```

This will:
1. Install `cargo-llvm-cov` if needed
2. Run tests with coverage
3. Generate LCOV format: `lcov.info`
4. Generate HTML report: `coverage-html/index.html`

### View Coverage
Open `coverage-html/index.html` in your browser.

## CI/CD Integration

### GitHub Actions
The CI workflow (`.github/workflows/rust-ci.yml`) includes:
- **rust-cache**: Caches dependencies for faster builds
- **Parallel execution**: Uses nextest for faster test runs
- **PostgreSQL service**: Provides test database
- **Coverage generation**: Uploads coverage reports as artifacts

### CI Stages
1. Format check (`cargo fmt`)
2. Clippy linting (`cargo clippy`)
3. Unit tests (with nextest)
4. Integration tests (if DATABASE_URL available)
5. Release build
6. Coverage report generation

## Test Dependencies

- **proptest**: Property-based testing
- **mockall**: Trait mocking
- **loom**: Concurrency testing
- **tower-test**: HTTP route testing
- **tokio-test**: Async testing utilities
- **criterion**: Benchmarking (for future use)

## Best Practices

1. **Unit Tests**: Fast, isolated, no external dependencies
2. **Integration Tests**: Test real interactions, use test database
3. **Concurrency Tests**: Use loom for async/threading edge cases
4. **Property-Based Tests**: Use proptest for comprehensive input validation

## Future Enhancements

- [ ] Add more API route integration tests
- [ ] Add E2E tests with testcontainers (if needed)
- [ ] Set up coverage thresholds in CI
- [ ] Add selective test runs based on git changes
