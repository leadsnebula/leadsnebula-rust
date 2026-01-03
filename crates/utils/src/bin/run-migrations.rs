use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::migrate::Migrator;
use sqlx::PgPool;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

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
    if !migrations_dir.exists() {
        // Try relative to crate root
        let crate_root = env!("CARGO_MANIFEST_DIR");
        let migrations_path = Path::new(crate_root)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("migrations");
        if migrations_path.exists() {
            run_migrations(&pool, &migrations_path).await?;
        } else {
            error!("Migrations directory not found");
            return Err(anyhow::anyhow!("Migrations directory not found"));
        }
    } else {
        run_migrations(&pool, migrations_dir).await?;
    }

    info!("Migrations completed successfully");

    Ok(())
}

async fn run_migrations(pool: &PgPool, migrations_dir: &Path) -> Result<()> {
    let migrator = Migrator::new(migrations_dir).await?;
    migrator.run(pool).await?;
    Ok(())
}
