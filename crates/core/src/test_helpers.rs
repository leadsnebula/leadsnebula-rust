use once_cell::sync::Lazy;
use sqlx::migrate::Migrator;
use sqlx::{postgres::PgPoolOptions, PgPool};
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
pub async fn create_test_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
    // Load environment files if present (non-fatal)
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL must be set for integration tests".to_string())?;

    if database_url.contains("://root@") || database_url.contains("://root:") {
        return Err(format!(
            "DATABASE_URL must not use 'root' user. Use 'postgres' instead. Current DATABASE_URL: {}",
            database_url
        )
        .into());
    }

    // Return cached pool if available
    {
        let cache = POOL_CACHE.lock().unwrap();
        if let Some(p) = cache.get(&database_url) {
            return Ok(p.clone());
        }
    }

    let max_conns: u32 = std::env::var("TEST_POOL_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let pool = PgPoolOptions::new()
        .max_connections(max_conns)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
        .map_err(|e| format!("Failed to connect to database: {}", e))?;

    // Quick health test
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|e| format!("Database connection test failed: {}", e))?;

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
            // Only run migrations if needed (check _sqlx_migrations)
            match should_run_migrations(&pool).await {
                Ok(true) => {
                    // Run migrations with a timeout and fail fast on errors
                    let migrator = Migrator::new(migrations_path.as_path()).await?;
                    let run = tokio::time::timeout(std::time::Duration::from_secs(60), async {
                        migrator.run(&pool).await
                    })
                    .await;

                    match run {
                        Ok(Ok(_)) => {
                            // success
                            let mut mc = MIGRATION_CACHE.lock().unwrap();
                            mc.insert(database_url.clone());
                        }
                        Ok(Err(e)) => {
                            eprintln!("Migrations failed: {}", e);
                            return Err(Box::new(e));
                        }
                        Err(_) => {
                            return Err("Migrations timed out after 60s".into());
                        }
                    }
                }
                Ok(false) => {
                    // Migrations not needed
                    let mut mc = MIGRATION_CACHE.lock().unwrap();
                    mc.insert(database_url.clone());
                }
                Err(e) => {
                    // If we can't determine migration state, err on the side of caution and run migrations
                    eprintln!("Could not check migration status: {}", e);
                    let migrator = Migrator::new(migrations_path.as_path()).await?;
                    let run = tokio::time::timeout(std::time::Duration::from_secs(60), async {
                        migrator.run(&pool).await
                    })
                    .await;

                    match run {
                        Ok(Ok(_)) => {
                            let mut mc = MIGRATION_CACHE.lock().unwrap();
                            mc.insert(database_url.clone());
                        }
                        Ok(Err(e)) => return Err(Box::new(e)),
                        Err(_) => return Err("Migrations timed out after 60s".into()),
                    }
                }
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

async fn should_run_migrations(pool: &PgPool) -> Result<bool, sqlx::Error> {
    // Check if the _sqlx_migrations table exists
    let table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;

    if table_count == 0 {
        // No table -> migrations should run
        return Ok(true);
    }

    // If the table exists, check if there are any applied migrations
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;
    Ok(applied == 0)
}

fn find_migrations_dir() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
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

    Err("Could not find migrations directory".into())
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
) -> Result<(PgPool, sqlx::Transaction<'static, sqlx::Postgres>), Box<dyn std::error::Error>> {
    let pool = create_test_pool().await?;
    let tx = pool.begin().await?;
    Ok((pool, tx))
}
