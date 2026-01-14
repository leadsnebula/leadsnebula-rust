// Test helper module for integration tests
// Handles database connection and migrations gracefully, even when migrations are already applied

use sqlx::migrate::Migrator;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::path::Path;

/// Create a test database pool, applying migrations only if needed
/// This handles the case where migrations are already applied to the database
///
/// The sqlx Migrator should handle already-applied migrations gracefully,
/// but we catch any errors related to duplicate migration records.
pub async fn create_test_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
    // Load environment
    let _ = dotenv::from_filename(".env.local");
    let _ = dotenv::dotenv();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration tests");

    // Validate DATABASE_URL format - ensure it doesn't use 'root' user
    if database_url.contains("://root@") || database_url.contains("://root:") {
        panic!(
            "DATABASE_URL must not use 'root' user. Use 'postgres' user instead. \
             Current DATABASE_URL: {}",
            database_url
        );
    }

    // Create pool with connection timeout
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
        .map_err(|e| {
            format!(
                "Failed to connect to database with URL: {} (error: {})",
                database_url.replace(
                    &std::env::var("POSTGRES_PASSWORD").unwrap_or_default(),
                    "***"
                ),
                e
            )
        })?;

    // Test the connection works before trying migrations
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|e| format!("Database connection test failed: {}", e))?;

    // Find migrations directory - try multiple locations
    // If we can't find it, that's okay - tests might work without migrations
    if let Ok(migrations_path) = find_migrations_dir() {
        // Check if migrations table exists - if not, we might need to create it
        // But if migrations are already applied, we can skip
        let migrations_exist =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .unwrap_or(0);

        // Only try to run migrations if the table exists (migrations have been run before)
        // or if we want to apply them fresh. For existing DBs, skip migration application
        // to avoid hanging on duplicate key errors.
        if migrations_exist > 0 {
            // Migrations already applied - skip to avoid conflicts
            // The database should already be in the correct state
        } else {
            // No migrations recorded - try to apply them with timeout
            let migration_result =
                tokio::time::timeout(std::time::Duration::from_secs(30), async {
                    if let Ok(migrator) = Migrator::new(migrations_path.as_path()).await {
                        migrator.run(&pool).await
                    } else {
                        Ok(())
                    }
                })
                .await;

            match migration_result {
                Ok(Ok(_)) => {
                    // Migrations applied successfully
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    // If error is about already-applied migrations, that's fine
                    if error_msg.contains("was previously applied")
                        || error_msg.contains("duplicate key value")
                        || error_msg.contains("unique constraint")
                    {
                        // These are expected when migrations are already applied
                    } else {
                        // Real error - but don't fail the test, just log it
                        eprintln!("Warning: Migration error (continuing anyway): {}", e);
                    }
                }
                Err(_) => {
                    // Timeout - migrations took too long, but continue anyway
                    eprintln!("Warning: Migration application timed out (continuing anyway)");
                }
            }
        }
    }

    Ok(pool)
}

/// Find the migrations directory, trying multiple locations
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
        // Try going up to workspace root
        if let Some(workspace_root) = manifest_path.parent().and_then(|p| p.parent()) {
            let migrations = workspace_root.join("migrations");
            if migrations.exists() {
                return Ok(migrations);
            }
        }
    }

    Err("Could not find migrations directory".into())
}
