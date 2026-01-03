use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use sqlx::migrate::Migrator;
use sqlx::PgPool;
use std::path::Path;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

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
