use anyhow::Result;
use leadsnebula_core::ssm::SsmService;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development (highest priority)
    // This ensures local development doesn't interfere with production
    let env_loaded = dotenvy::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenvy::dotenv();
    }

    if env_loaded {
        tracing::info!("Loaded environment from .env.local (local development mode)");
    }

    let environment = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("ENV"))
        .unwrap_or_else(|_| "development".to_string());

    // Prefer DATABASE_URL from env (CI/Neon ephemeral, local .env.local) so migrations run without SSM.
    // Only call SSM when DATABASE_URL is not set (e.g. production/Fly.io).
    let database_url = if let Ok(url) = std::env::var("DATABASE_URL") {
        url
    } else {
        let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);
        let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);
        let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
        let params = ssm.get_parameters_by_path(&config_path).await?;
        params
            .get(&format!(
                "/leadsnebula/{}/rust/db/connection_url",
                env_normalized
            ))
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "DATABASE_URL not set and not found in SSM at /leadsnebula/{}/rust/db/connection_url",
                    env_normalized
                )
            })?
    };

    // Use a simpler pool configuration for migrations (only need 1 connection).
    // Set statement_timeout high so migrations and cleanup don't hit server default (e.g. Neon 60s).
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .min_connections(0) // Don't pre-establish connections for migrations
        .acquire_timeout(Duration::from_secs(10))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '300000'") // 5 minutes in ms
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&database_url)
        .await?;

    info!("Running database migrations...");

    // Find migrations directory
    let migrations_dir = Path::new("migrations");
    let migrations_path = if !migrations_dir.exists() {
        // Try relative to crate root
        let crate_root = env!("CARGO_MANIFEST_DIR");
        Path::new(crate_root)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("migrations")
    } else {
        migrations_dir.to_path_buf()
    };

    // Check if migrations directory exists and has migration files
    if !migrations_path.exists() {
        info!("Migrations directory not found, skipping migrations");
        return Ok(());
    }

    // Check if there are any migration files (excluding .gitkeep)
    let has_migrations = std::fs::read_dir(&migrations_path)?
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            let path = entry.path();
            path.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".sql"))
                    .unwrap_or(false)
        });

    if !has_migrations {
        info!(
            "No migration files found in {}, skipping migrations",
            migrations_path.display()
        );
        return Ok(());
    }

    // Check if we're in test mode (for ephemeral test databases)
    // Test mode is enabled if:
    // 1. TEST_MODE env var is set
    // 2. NEON_BRANCH env var is set (ephemeral branch)
    // 3. Database URL contains test-like patterns
    // 4. We're running in a test environment (CI, local dev with ephemeral branches)
    let test_mode = std::env::var("TEST_MODE").is_ok() 
        || std::env::var("NEON_BRANCH").is_ok()
        || database_url.contains("ci-local-")
        || database_url.contains("test-")
        || database_url.contains("ep-")  // Neon endpoint IDs often start with ep-
        || environment == "development"  // Development environment is safe for test mode
        || environment == "test";

    run_migrations(&pool, &migrations_path, test_mode).await?;

    info!("Migrations completed successfully");

    Ok(())
}

async fn run_migrations(pool: &PgPool, migrations_dir: &Path, _test_mode: bool) -> Result<()> {
    // Proactively clean up orphaned migration records before creating Migrator
    // This prevents "migration was previously applied but is missing" errors
    if let Err(e) = cleanup_inconsistent_migrations(pool, migrations_dir).await {
        info!("Warning: Could not proactively clean up migrations: {}", e);
    }

    // Create migrator - SQLx will validate migration files match database state.
    // Loop: if "modified" migrations are detected, remove their records and retry (handles multiple modified).
    const MAX_MIGRATOR_RETRIES: u32 = 25;
    let mut attempts = 0u32;
    let migrator = loop {
        match Migrator::new(migrations_dir).await {
            Ok(m) => break m,
            Err(e) => {
                attempts += 1;
                let error_msg = e.to_string();

                if error_msg.contains("was previously applied but is missing") {
                    return Err(anyhow::anyhow!(
                        "Database has migration records for files that no longer exist. \
                        Error: {}. \
                        To fix: restore the missing migration file or remove the orphaned record from _sqlx_migrations.",
                        error_msg
                    ));
                }

                if error_msg.contains("was previously applied but has been modified") {
                    if attempts > MAX_MIGRATOR_RETRIES {
                        return Err(anyhow::anyhow!(
                            "Database has migration records for files that have been modified. \
                            Auto-retry failed after {} attempts. Error: {}",
                            MAX_MIGRATOR_RETRIES,
                            error_msg
                        ));
                    }
                    let modified_versions = extract_all_modified_migration_versions(&error_msg);
                    if modified_versions.is_empty() {
                        return Err(anyhow::anyhow!(
                            "Database has migration records for files that have been modified. Error: {}",
                            error_msg
                        ));
                    }
                    for version in &modified_versions {
                        info!(
                            "Removing stale record for modified migration {}...",
                            version
                        );
                        if let Err(err) =
                            sqlx::query("DELETE FROM _sqlx_migrations WHERE version = $1")
                                .bind(version)
                                .execute(pool)
                                .await
                        {
                            info!("Warning: Could not remove migration {}: {}", version, err);
                        }
                    }
                    if let Err(cleanup_err) =
                        cleanup_inconsistent_migrations(pool, migrations_dir).await
                    {
                        info!("Warning: Could not clean up migrations: {}", cleanup_err);
                    }
                    continue;
                }

                return Err(anyhow::anyhow!("Failed to create migrator: {}", e));
            }
        }
    };

    // Run migrations
    run_migrations_inner(pool, migrator).await
}

/// Check for pending migrations
async fn check_pending_migrations(
    pool: &PgPool,
    migrator: &Migrator,
) -> Result<Vec<(i64, String)>> {
    use std::collections::HashSet;

    // Get applied migrations from database
    let applied_versions: HashSet<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = true")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();

    // Get all migrations from migrator
    let mut pending = Vec::new();
    for migration in migrator.iter() {
        let version = migration.version;
        let name = migration.description.to_string();
        if !applied_versions.contains(&version) {
            pending.push((version, name));
        }
    }

    Ok(pending)
}

async fn run_migrations_inner(pool: &PgPool, migrator: Migrator) -> Result<()> {
    // Check for pending migrations before running
    let pending = check_pending_migrations(pool, &migrator).await?;

    if pending.is_empty() {
        info!("✅ No pending migrations - database is up to date");
        return Ok(());
    }

    info!(
        "⚠️  Found {} pending migration(s), applying now...",
        pending.len()
    );
    for (version, name) in &pending {
        info!("   - {} (version: {})", name, version);
    }

    // Run migrations - SQLx will automatically skip already-applied ones
    match migrator.run(pool).await {
        Ok(_) => {
            // Verify all pending migrations were applied
            let still_pending = check_pending_migrations(pool, &migrator).await?;
            if !still_pending.is_empty() {
                return Err(anyhow::anyhow!(
                    "Migration run completed but {} migration(s) are still pending: {:?}",
                    still_pending.len(),
                    still_pending
                ));
            }
            info!("✅ All {} migration(s) applied successfully", pending.len());
            Ok(())
        }
        Err(e) => {
            let error_msg = e.to_string();
            if error_msg.contains("was previously applied but is missing") {
                Err(anyhow::anyhow!(
                    "Database has migration records for files that no longer exist. \
                    This indicates a migration file was deleted after being applied. \
                    Error: {}. \
                    To fix: either restore the missing migration file or manually remove \
                    the orphaned record from _sqlx_migrations table.",
                    error_msg
                ))
            } else if error_msg.contains("was previously applied but has been modified") {
                // Loop: remove stale checksum record(s) for modified migration(s), then retry run().
                const MAX_RUN_RETRIES: u32 = 25;
                let mut run_attempts = 0u32;
                let mut last_error_msg = error_msg.clone();
                loop {
                    run_attempts += 1;
                    let modified_versions =
                        extract_all_modified_migration_versions(&last_error_msg);
                    if modified_versions.is_empty() {
                        return Err(anyhow::anyhow!(
                            "Database has migration records for files that have been modified. \
                            Error: {}. \
                            To fix: revert the migration file or create a new migration.",
                            last_error_msg
                        ));
                    }
                    if run_attempts > MAX_RUN_RETRIES {
                        return Err(anyhow::anyhow!(
                            "Database has migration records for files that have been modified and auto-retry failed after {} attempts. \
                            Last error: {}",
                            MAX_RUN_RETRIES,
                            last_error_msg
                        ));
                    }
                    for version in &modified_versions {
                        info!(
                            "Detected modified migration {}. Removing stale record and retrying...",
                            version
                        );
                        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = $1")
                            .bind(version)
                            .execute(pool)
                            .await?;
                    }
                    match migrator.run(pool).await {
                        Ok(_) => {
                            let still_pending = check_pending_migrations(pool, &migrator).await?;
                            if !still_pending.is_empty() {
                                return Err(anyhow::anyhow!(
                                    "Migration retry completed but {} migration(s) are still pending: {:?}",
                                    still_pending.len(),
                                    still_pending
                                ));
                            }
                            info!("✅ Migration run succeeded after stale record cleanup");
                            return Ok(());
                        }
                        Err(retry_err) => {
                            last_error_msg = retry_err.to_string();
                            if !last_error_msg
                                .contains("was previously applied but has been modified")
                            {
                                return Err(anyhow::anyhow!(
                                    "Migration run failed after cleanup: {}",
                                    last_error_msg
                                ));
                            }
                            continue;
                        }
                    }
                }
            } else {
                Err(anyhow::anyhow!("Migration failed: {}", e))
            }
        }
    }
}

/// Extract all migration version numbers mentioned in a "was previously applied but has been modified" error.
/// Handles messages that mention one or more versions (e.g. after a retry).
fn extract_all_modified_migration_versions(error_msg: &str) -> Vec<i64> {
    let mut versions = Vec::new();
    // Pattern: "migration 20250101000007 was ..." - collect every "migration <number>"
    let mut search_start = 0;
    while let Some(offset) = error_msg[search_start..].find("migration ") {
        let start = search_start + offset + 10; // after "migration "
        let rest = &error_msg[start..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if !rest[..end].is_empty() {
            if let Ok(v) = rest[..end].parse::<i64>() {
                versions.push(v);
            }
        }
        search_start = start + end;
        if search_start >= error_msg.len() {
            break;
        }
    }
    if versions.is_empty() {
        // Fallback: first long integer (e.g. 14 digits)
        if let Some(v) = error_msg
            .split(|c: char| !c.is_ascii_digit())
            .find_map(|token| {
                if token.len() >= 8 {
                    token.parse::<i64>().ok()
                } else {
                    None
                }
            })
        {
            versions.push(v);
        }
    }
    versions
}

/// Clean up inconsistent migration records (for test mode only)
async fn cleanup_inconsistent_migrations(pool: &PgPool, migrations_dir: &Path) -> Result<()> {
    use std::collections::HashSet;

    // Get list of all migration files
    let mut file_versions = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(migrations_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(version_str) = name.split('_').next() {
                    if let Ok(version) = version_str.parse::<i64>() {
                        file_versions.insert(version);
                    }
                }
            }
        }
    }

    // Get applied migrations from database
    let applied_versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await?;

    // Find orphaned migrations (applied but file missing)
    let orphaned: Vec<i64> = applied_versions
        .iter()
        .filter(|v| !file_versions.contains(v))
        .copied()
        .collect();

    // Remove orphaned migration records
    if !orphaned.is_empty() {
        info!(
            "Cleaning up {} orphaned migration record(s): {:?}",
            orphaned.len(),
            orphaned
        );
        for version in &orphaned {
            sqlx::query("DELETE FROM _sqlx_migrations WHERE version = $1")
                .bind(version)
                .execute(pool)
                .await?;
        }
    }

    // Note: Modified migrations are detected by SQLx during Migrator::new()
    // They will be handled in the error path by extracting the version from the error message

    Ok(())
}
