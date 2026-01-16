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
    // In CI, Neon can be slow, so we need more headroom and longer timeouts
    let max_conns: u32 = std::env::var("TEST_POOL_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20); // Increased from 10 to 20 for CI headroom

    let pool = PgPoolOptions::new()
        .max_connections(max_conns)
        .acquire_timeout(std::time::Duration::from_secs(30)) // Increased from 10s to 30s for Neon CI slowness
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
                let is_test_mode = database_url.contains("ci-local-")
                    || database_url.contains("test-")
                    || database_url.contains("ep-")
                    || std::env::var("TEST_MODE").is_ok()
                    || std::env::var("NEON_BRANCH").is_ok();

                if is_test_mode {
                    // Drop and recreate _sqlx_migrations table to ensure clean state
                    // This prevents checksum mismatches from copied branch state
                    sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations CASCADE")
                        .execute(&pool)
                        .await
                        .ok();

                    // Recreate table with exact SQLx schema
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
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to create _sqlx_migrations table: {}", e)
                    })?;

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
