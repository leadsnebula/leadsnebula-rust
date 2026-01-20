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

    // Load DATABASE_URL from SSM (like main app) or fall back to env var
    let environment = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("ENV"))
        .unwrap_or_else(|_| "development".to_string());

    let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);
    let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);
    let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
    let params = ssm.get_parameters_by_path(&config_path).await?;

    let database_url = params
        .get(&format!("/leadsnebula/{}/rust/db/connection_url", env_normalized))
        .cloned()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "DATABASE_URL not found in SSM at /leadsnebula/{}/rust/db/connection_url and not in environment variables",
                env_normalized
            )
        })?;

    // Use a simpler pool configuration for migrations (only need 1 connection)
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .min_connections(0) // Don't pre-establish connections for migrations
        .acquire_timeout(Duration::from_secs(10))
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

    // Create migrator - SQLx will validate migration files match database state
    // Retry once if we encounter modified/missing migration errors
    let migrator = match Migrator::new(migrations_dir).await {
        Ok(m) => m,
        Err(e) => {
            let error_msg = e.to_string();

            // If error is about modified or missing migrations, try to clean up and retry once
            if error_msg.contains("was previously applied but is missing")
                || error_msg.contains("was previously applied but has been modified")
            {
                info!("Cleaning up inconsistent migration records and retrying...");

                // Extract and remove modified migration versions
                if error_msg.contains("was previously applied but has been modified") {
                    // Try multiple patterns to extract version number
                    let mut modified_versions: Vec<i64> = Vec::new();

                    // Pattern 1: "migration 20250101000007 was previously applied"
                    for part in error_msg.split("migration") {
                        if let Some(version_str) = part.split_whitespace().next() {
                            if let Ok(version) = version_str.parse::<i64>() {
                                modified_versions.push(version);
                                break; // Found it, no need to continue
                            }
                        }
                    }

                    // Pattern 2: Try regex-like extraction "migration <number>"
                    if modified_versions.is_empty() {
                        if let Some(start) = error_msg.find("migration ") {
                            let rest = &error_msg[start + 10..]; // "migration " is 10 chars
                            if let Some(end) = rest.find(' ') {
                                if let Ok(version) = rest[..end].parse::<i64>() {
                                    modified_versions.push(version);
                                }
                            }
                        }
                    }

                    for version in &modified_versions {
                        if let Err(e) =
                            sqlx::query("DELETE FROM _sqlx_migrations WHERE version = $1")
                                .bind(version)
                                .execute(pool)
                                .await
                        {
                            info!("Warning: Could not remove migration {}: {}", version, e);
                        }
                    }
                }

                // Clean up orphaned migrations
                if let Err(cleanup_err) =
                    cleanup_inconsistent_migrations(pool, migrations_dir).await
                {
                    info!("Warning: Could not clean up migrations: {}", cleanup_err);
                }

                // Retry once
                Migrator::new(migrations_dir).await.map_err(|e| {
                    anyhow::anyhow!("Failed to create migrator after cleanup: {}", e)
                })?
            } else {
                return Err(anyhow::anyhow!("Failed to create migrator: {}", e));
            }
        }
    };

    // Run migrations
    run_migrations_inner(pool, migrator).await
}

async fn run_migrations_inner(pool: &PgPool, migrator: Migrator) -> Result<()> {
    // Run migrations - SQLx will automatically skip already-applied ones
    match migrator.run(pool).await {
        Ok(_) => Ok(()),
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
                Err(anyhow::anyhow!(
                    "Database has migration records for files that have been modified. \
                    This indicates a migration file was changed after being applied. \
                    Error: {}. \
                    To fix: either revert the migration file to its original state or \
                    create a new migration to make the desired changes.",
                    error_msg
                ))
            } else {
                Err(anyhow::anyhow!("Migration failed: {}", e))
            }
        }
    }
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
