use anyhow::{Context, Result};
use leadsnebula_core::SsmClient;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load .env.local for local development
    let _ = dotenvy::from_filename(".env.local").ok();

    // Get environment
    let environment = std::env::var("ENVIRONMENT")
        .unwrap_or_else(|_| std::env::var("ENV").unwrap_or_else(|_| "development".to_string()));

    info!("Running migrations for environment: {}", environment);

    // Get database URL from SSM or environment
    let database_url = load_database_url(&environment)
        .await
        .context("Failed to load database URL")?;

    info!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    // Find migrations directory
    let migrations_dir = find_migrations_dir().context("Failed to find migrations directory")?;
    info!("Using migrations directory: {}", migrations_dir);

    // Use sqlx's built-in Migrator - handles everything automatically
    // This is the proper, idiomatic way to run migrations with sqlx
    // Migrator::new accepts a path string or Path that implements MigrationSource
    let migrator = Migrator::new(Path::new(&migrations_dir))
        .await
        .context("Failed to initialize migrator")?;

    info!("Running migrations...");
    migrator
        .run(&pool)
        .await
        .context("Failed to run migrations")?;

    info!("Migrations completed successfully!");
    Ok(())
}

fn find_migrations_dir() -> Result<String> {
    // Try common locations in order of preference
    let possible_paths = [
        "/app/migrations", // Fly.io deployment
        "./migrations",    // Local development (from workspace root)
        "../migrations",   // Alternative local path
    ];

    for path_str in &possible_paths {
        let path = Path::new(path_str);
        if path.exists() && path.is_dir() {
            return Ok(path_str.to_string());
        }
    }

    Err(anyhow::anyhow!(
        "Migrations directory not found. Tried: {}",
        possible_paths.join(", ")
    ))
}

async fn load_database_url(environment: &str) -> Result<String> {
    // Try SSM first (production/staging)
    let ssm_client = match SsmClient::new().await {
        Ok(client) => Arc::new(client),
        Err(e) => {
            tracing::warn!(
                "SSM client initialization failed: {}. Falling back to environment variables.",
                e
            );
            Arc::new(SsmClient::dummy())
        }
    };

    let param_path = format!("/leadsnebula/{}/rust/db/connection_url", environment);

    if let Some(url) = ssm_client.get_parameter(&param_path).await? {
        return Ok(url);
    }

    // Fall back to environment variable (local development)
    std::env::var("DATABASE_URL").context(
        "DATABASE_URL not found in SSM or environment variables. \
         Set DATABASE_URL environment variable or configure SSM parameter.",
    )
}
