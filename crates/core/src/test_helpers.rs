use anyhow;
use once_cell::sync::Lazy;
use sqlx::migrate::Migrator;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

/// In-memory caches to avoid re-running migrations and re-creating pools repeatedly during tests.
static POOL_CACHE: Lazy<Mutex<HashMap<String, PgPool>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static MIGRATION_CACHE: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Creates or returns a pooled connection to the test database.
/// - Loads `.env.local` and environment variables
/// - Connects to `DATABASE_URL`
/// - Checks `_sqlx_migrations` and only runs migrations if needed (cached)
/// - Fails fast on migration errors
///
/// Note: `PgPool::clone()` is cheap (Arc-based), so cached pools are efficiently shared.
pub async fn create_test_pool() -> anyhow::Result<PgPool> {
    // Load environment files if present (non-fatal)
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set for integration tests"))?;

    if database_url.contains("://root@") || database_url.contains("://root:") {
        return Err(anyhow::anyhow!(
            "DATABASE_URL must not use 'root' user. Use 'postgres' instead. Current DATABASE_URL: {}",
            database_url
        ));
    }

    // CRITICAL: Refuse connections to main DB - tests must use ephemeral branches only
    // This prevents test data from polluting the main database
    let is_ci = std::env::var("CI").is_ok();
    let is_ephemeral = std::env::var("EPHEMERAL_DB").is_ok();
    let looks_like_ephemeral = database_url.contains("ci-") || database_url.contains("ci-local-");

    if !is_ci && !is_ephemeral && !looks_like_ephemeral {
        return Err(anyhow::anyhow!(
            "❌ REFUSED: Tests cannot run against main database.\n\
             \n\
             To run tests:\n\
             1. Use ephemeral Neon branch: ./autotests.sh\n\
             2. Or set EPHEMERAL_DB=1 with an ephemeral DATABASE_URL\n\
             3. Or run in CI (CI=1 is set automatically)\n\
             \n\
             Current DATABASE_URL appears to be main DB (not ephemeral).\n\
             This prevents test data from polluting your main database.\n\
             \n\
             If you need to run tests locally, use: ./autotests.sh"
        ));
    }

    // Return cached pool if available
    // Note: PgPool::clone() is cheap (Arc-based), so this is efficient
    {
        let cache = POOL_CACHE.lock().unwrap();
        if let Some(p) = cache.get(&database_url) {
            return Ok(p.clone());
        }
    }

    // Increase default pool size for tests to handle concurrent test execution
    // Tests run with --test-threads=1, but transactions can hold connections longer
    // Concurrency tests (duplicate_post spawns 10 concurrent tasks) need many connections
    // In CI, Neon free-tier can be very slow, requiring much larger pools and timeouts
    let is_ci = std::env::var("CI").is_ok();
    let max_conns: u32 = std::env::var("TEST_POOL_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if is_ci {
            50 // CI: Much larger pool for Neon free-tier slowness + concurrent tests
        } else {
            30 // Local: 30 is sufficient for concurrency tests (duplicate_post spawns 10 tasks)
        });

    let acquire_timeout_secs = std::env::var("TEST_POOL_ACQUIRE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if is_ci {
            120 // CI: Much longer timeout for Neon free-tier cold starts and slowness
        } else {
            60 // Local: 60s is sufficient for concurrency tests
        });

    let pool = PgPoolOptions::new()
        .max_connections(max_conns)
        .acquire_timeout(std::time::Duration::from_secs(acquire_timeout_secs))
        .connect(&database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?;

    // Quick health test
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Database connection test failed: {}", e))?;

    // Find migrations directory
    if let Ok(migrations_path) = find_migrations_dir() {
        // Determine test mode early - needed for table creation logic
        let is_test_mode = database_url.contains("ci-local-")
            || database_url.contains("test-")
            || std::env::var("TEST_MODE").is_ok()
            || std::env::var("NEON_BRANCH").is_ok()
            || std::env::var("CI").is_ok(); // CI=1 is set in GitHub Actions

        // ALWAYS ensure _sqlx_migrations table exists, regardless of cache
        // This is critical - sqlx::migrate requires this table to exist
        // Even if cache says migrations have been run, the database might be fresh
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success BOOLEAN NOT NULL,
                checksum BYTEA NOT NULL,
                execution_time BIGINT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to ensure _sqlx_migrations table exists: {}", e))?;

        // Verify the table exists by querying it (ensures it's committed and visible)
        // Retry with exponential backoff to handle race conditions when multiple tests
        // create the table concurrently or when there are visibility delays
        let mut retries = 0;
        let max_retries = 5;
        loop {
            match sqlx::query("SELECT 1 FROM _sqlx_migrations LIMIT 1")
                .fetch_optional(&pool)
                .await
            {
                Ok(_) => break, // Table exists and is visible
                Err(e) if retries < max_retries => {
                    // Check if it's a "does not exist" error (relation not found)
                    let error_str = e.to_string();
                    if error_str.contains("does not exist") || error_str.contains("relation") {
                        retries += 1;
                        let delay_ms = 50 * (1 << retries); // 100ms, 200ms, 400ms, 800ms, 1600ms
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                        // Try recreating the table in case it was dropped by another concurrent test
                        let _ = sqlx::query(
                            "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
                                version BIGINT PRIMARY KEY,
                                description TEXT NOT NULL,
                                installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                                success BOOLEAN NOT NULL,
                                checksum BYTEA NOT NULL,
                                execution_time BIGINT NOT NULL
                            )",
                        )
                        .execute(&pool)
                        .await;
                        continue;
                    } else {
                        // Different error - fail immediately
                        return Err(anyhow::anyhow!(
                            "Failed to verify _sqlx_migrations table exists: {}",
                            e
                        ));
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to verify _sqlx_migrations table exists after {} retries: {}",
                        max_retries,
                        e
                    ));
                }
            }
        }

        let mut should_run = true;
        {
            let mc = MIGRATION_CACHE.lock().unwrap();
            if mc.contains(&database_url) {
                should_run = false;
            }
        }

        if should_run {
            // Atomic check-and-set: Check migration cache again after acquiring lock
            // This prevents race conditions where multiple tests try to run migrations simultaneously
            let needs_migration = {
                let mc = MIGRATION_CACHE.lock().unwrap();
                !mc.contains(&database_url)
            };

            if needs_migration {
                // Mark as in-progress to prevent other threads from running migrations
                {
                    let mut mc = MIGRATION_CACHE.lock().unwrap();
                    mc.insert(database_url.clone());
                } // Release lock before async operations

                // Simple, clean migration logic for test mode
                // In test mode (ephemeral Neon branches), always drop and recreate _sqlx_migrations
                // This ensures a clean slate and prevents checksum mismatches from copied state
                // NOTE: "ep-" in URL is Neon endpoint naming, NOT a test indicator - only check for actual test prefixes
                // CI environment variable indicates we're in CI (GitHub Actions, etc.) and should treat as test mode

                if is_test_mode {
                    // In test mode, drop and recreate _sqlx_migrations table to ensure clean state
                    // This prevents checksum mismatches from copied branch state
                    // Remove CASCADE to avoid dropping dependent types that might cause "type already exists" errors
                    sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations")
                        .execute(&pool)
                        .await
                        .ok();

                    // Small delay to ensure DROP is committed before CREATE
                    // This helps avoid race conditions when multiple tests run concurrently
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    // Recreate table with exact SQLx schema
                    // Use CREATE TABLE (not IF NOT EXISTS) since we just dropped it
                    sqlx::query(
                        "CREATE TABLE _sqlx_migrations (
                            version BIGINT PRIMARY KEY,
                            description TEXT NOT NULL,
                            installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                            success BOOLEAN NOT NULL,
                            checksum BYTEA NOT NULL,
                            execution_time BIGINT NOT NULL
                        )",
                    )
                    .execute(&pool)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to recreate _sqlx_migrations table in test mode: {}",
                            e
                        )
                    })?;

                    // Verify the table exists after recreation
                    // Retry with exponential backoff to handle race conditions
                    let mut retries = 0;
                    let max_retries = 5;
                    loop {
                        match sqlx::query("SELECT 1 FROM _sqlx_migrations LIMIT 1")
                            .fetch_optional(&pool)
                            .await
                        {
                            Ok(_) => break, // Table exists and is visible
                            Err(e) if retries < max_retries => {
                                let error_str = e.to_string();
                                if error_str.contains("does not exist")
                                    || error_str.contains("relation")
                                {
                                    retries += 1;
                                    let delay_ms = 50 * (1 << retries); // 100ms, 200ms, 400ms, 800ms, 1600ms
                                    tokio::time::sleep(tokio::time::Duration::from_millis(
                                        delay_ms,
                                    ))
                                    .await;
                                    // Try recreating the table in case it was dropped by another concurrent test
                                    let _ = sqlx::query(
                                        "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
                                            version BIGINT PRIMARY KEY,
                                            description TEXT NOT NULL,
                                            installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                                            success BOOLEAN NOT NULL,
                                            checksum BYTEA NOT NULL,
                                            execution_time BIGINT NOT NULL
                                        )",
                                    )
                                    .execute(&pool)
                                    .await;
                                    continue;
                                } else {
                                    return Err(anyhow::anyhow!(
                                        "Failed to verify _sqlx_migrations table exists after recreation: {}",
                                        e
                                    ));
                                }
                            }
                            Err(e) => {
                                return Err(anyhow::anyhow!(
                                    "Failed to verify _sqlx_migrations table exists after recreation (after {} retries): {}",
                                    max_retries,
                                    e
                                ));
                            }
                        }
                    }

                    // Clean up partially applied migrations - fix inconsistent database state
                    // This handles cases where tables exist but are missing required columns
                    // (e.g., from partial migrations in copied Neon branches)

                    // Fix user_otp_settings table - drop if missing instance_user_id column
                    let user_otp_fix = sqlx::query(
                        "SELECT EXISTS (
                            SELECT 1 FROM information_schema.tables 
                            WHERE table_name = 'user_otp_settings'
                        ) AND NOT EXISTS (
                            SELECT 1 FROM information_schema.columns 
                            WHERE table_name = 'user_otp_settings' AND column_name = 'instance_user_id'
                        )"
                    )
                    .fetch_one(&pool)
                    .await;

                    if let Ok(row) = user_otp_fix {
                        let needs_fix: bool = row.get(0);
                        if needs_fix {
                            sqlx::query("DROP TABLE IF EXISTS user_otp_settings CASCADE")
                                .execute(&pool)
                                .await
                                .ok();
                        }
                    }

                    // Fix webauthn_credentials table - drop if missing instance_user_id column
                    let webauthn_fix = sqlx::query(
                        "SELECT EXISTS (
                            SELECT 1 FROM information_schema.tables 
                            WHERE table_name = 'webauthn_credentials'
                        ) AND NOT EXISTS (
                            SELECT 1 FROM information_schema.columns 
                            WHERE table_name = 'webauthn_credentials' AND column_name = 'instance_user_id'
                        )"
                    )
                    .fetch_one(&pool)
                    .await;

                    if let Ok(row) = webauthn_fix {
                        let needs_fix: bool = row.get(0);
                        if needs_fix {
                            sqlx::query("DROP TABLE IF EXISTS webauthn_credentials CASCADE")
                                .execute(&pool)
                                .await
                                .ok();
                        }
                    }

                    // Fix any tables with invalid foreign key constraints
                    sqlx::query(
                        "DO $$ 
                        DECLARE
                            r RECORD;
                        BEGIN
                            FOR r IN 
                                SELECT conname, conrelid::regclass::text as table_name
                                FROM pg_constraint
                                WHERE contype = 'f'
                                AND NOT EXISTS (
                                    SELECT 1 FROM pg_attribute a
                                    JOIN pg_constraint c ON a.attrelid = c.conrelid
                                    WHERE c.oid = pg_constraint.oid
                                    AND a.attnum = ANY(pg_constraint.conkey)
                                )
                            LOOP
                                EXECUTE format('ALTER TABLE %I DROP CONSTRAINT IF EXISTS %I CASCADE', r.table_name, r.conname);
                            END LOOP;
                        END $$;"
                    )
                    .execute(&pool)
                    .await
                    .ok();
                }

                // Check if migrations are already applied before running (optimization to avoid 45+ second delays)
                // The _sqlx_migrations table should exist at this point (created above)
                let migrations_check = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM _sqlx_migrations WHERE success = true",
                    )
                    .fetch_one(&pool),
                )
                .await;
                let migrations_already_applied = match migrations_check {
                    Ok(Ok(count)) => count > 0,
                    _ => false, // If query fails or times out, assume migrations not applied
                };

                if !migrations_already_applied {
                    // Run migrations - simple and clean, no retries needed after reset
                    let migrator = Migrator::new(migrations_path.as_path())
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to create migrator: {}", e))?;

                    migrator.run(&pool).await.map_err(|e| {
                        let error_msg = e.to_string();
                        // In test mode, provide helpful error message for "already exists" errors
                        if is_test_mode && (error_msg.contains("already exists") || error_msg.contains("duplicate key")) {
                            anyhow::anyhow!(
                                "Migration failed: {}. This happens when database objects already exist from copied branch state. \
                                Consider using 'IF NOT EXISTS' clauses in migration SQL files for test environments.",
                                error_msg
                            )
                        } else {
                            anyhow::anyhow!("Migration failed: {}", e)
                        }
                    })?;
                }
                // Migration succeeded - cache already updated above
            }
        }
    }

    // Cache pool for reuse
    {
        let mut cache = POOL_CACHE.lock().unwrap();
        cache.insert(database_url.clone(), pool.clone());
    }

    Ok(pool)
}

/// Retry a pool operation with exponential backoff to handle PoolTimedOut errors
/// This is especially useful in CI where Neon free-tier can be slow
/// The closure is called fresh on each retry, so it can recreate queries with bound values
pub async fn retry_pool_operation<F, Fut, T>(mut operation: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, sqlx::Error>>,
{
    let mut retries = 0;
    let max_retries = 5;
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(sqlx::Error::PoolTimedOut) if retries < max_retries => {
                retries += 1;
                let delay_ms = 200 * (1 << retries); // 400ms, 800ms, 1600ms, 3200ms, 6400ms
                eprintln!(
                    "PoolTimedOut on attempt {}, retrying in {}ms...",
                    retries, delay_ms
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                continue;
            }
            Err(e) => return Err(anyhow::anyhow!("Pool operation failed: {}", e)),
        }
    }
}

fn find_migrations_dir() -> anyhow::Result<std::path::PathBuf> {
    // Try current directory first
    let migrations_dir = Path::new("migrations");
    if migrations_dir.exists() {
        return Ok(migrations_dir.to_path_buf());
    }

    // Try parent directory (for tests in tests/ directory)
    let parent_migrations = Path::new("../migrations");
    if parent_migrations.exists() {
        return Ok(parent_migrations.to_path_buf());
    }

    // Try workspace root (for tests in crates/api/tests/)
    let workspace_migrations = Path::new("../../migrations");
    if workspace_migrations.exists() {
        return Ok(workspace_migrations.to_path_buf());
    }

    // Try using CARGO_MANIFEST_DIR if available
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_path = Path::new(&manifest_dir);
        if let Some(workspace_root) = manifest_path.parent().and_then(|p| p.parent()) {
            let migrations = workspace_root.join("migrations");
            if migrations.exists() {
                return Ok(migrations);
            }
        }
    }

    Err(anyhow::anyhow!("Could not find migrations directory"))
}

#[cfg(test)]
pub(crate) fn clear_test_caches() {
    let mut pc = POOL_CACHE.lock().unwrap();
    pc.clear();
    let mut mc = MIGRATION_CACHE.lock().unwrap();
    mc.clear();
}

#[cfg(test)]
pub(crate) fn is_migration_cached(database_url: &str) -> bool {
    let mc = MIGRATION_CACHE.lock().unwrap();
    mc.contains(database_url)
}

/// Create a test pool and begin a transaction
pub async fn create_test_pool_with_transaction(
) -> anyhow::Result<(PgPool, sqlx::Transaction<'static, sqlx::Postgres>)> {
    let pool = create_test_pool().await?;
    let tx = pool.begin().await?;
    Ok((pool, tx))
}

/// Check if heavy tests should run
/// Heavy tests are skipped by default in CI and fast iteration mode
/// Check if heavy tests should run
/// Relies on RUN_HEAVY_TESTS environment variable (set by autotests.sh)
/// autotests.sh exports RUN_HEAVY_TESTS=true by default for local runs
/// CI does not set this variable, so heavy tests are skipped in CI
pub fn should_run_heavy_tests() -> bool {
    std::env::var("RUN_HEAVY_TESTS")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}
