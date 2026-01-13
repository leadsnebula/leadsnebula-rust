use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::migrate::Migrator;
use sqlx::PgPool;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development (highest priority)
    // This ensures local development doesn't interfere with production
    let env_loaded = dotenv::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenv::dotenv();
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

    let pool = create_pool(&database_url).await?;

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

    run_migrations(&pool, &migrations_path).await?;

    info!("Migrations completed successfully");

    Ok(())
}

async fn run_migrations(pool: &PgPool, migrations_dir: &Path) -> Result<()> {
    let migrator = match Migrator::new(migrations_dir).await {
        Ok(m) => m,
        Err(e) => {
            // Handle case where database has migrations that don't exist in files
            // or have been modified since they were applied. Either situation
            // is acceptable for local/dev environments where migrations may
            // be reworked during development. In those cases skip running
            // the migrator rather than failing the process.
            let error_msg = e.to_string();
            if error_msg.contains("was previously applied but is missing") {
                info!(
                    "Database has migration records that don't match files. This is expected if migrations were removed. Skipping migration run."
                );
                return Ok(());
            }
            if error_msg.contains("was previously applied but has been modified") {
                info!(
                    "Database has migrations that were previously applied but the files were modified. Skipping migration run."
                );
                return Ok(());
            }
            return Err(anyhow::anyhow!("Failed to create migrator: {}", e));
        }
    };

    match migrator.run(pool).await {
        Ok(_) => Ok(()),
        Err(e) => {
            let error_msg = e.to_string();
            if error_msg.contains("was previously applied but is missing") {
                info!(
                    "Database has migration records that don't match files. This is expected if migrations were removed. Skipping migration run."
                );
                Ok(())
            } else if error_msg.contains("was previously applied but has been modified") {
                info!(
                    "Database has migrations that were previously applied but the files were modified. Skipping migration run."
                );
                Ok(())
            } else {
                Err(anyhow::anyhow!("Migration failed: {}", e))
            }
        }
    }
}
