use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use std::fs;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development
    let env_loaded = dotenvy::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenvy::dotenv();
    }

    // Load DATABASE_URL from SSM or env var
    let environment = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("ENV"))
        .unwrap_or_else(|_| "development".to_string());

    let env_normalized = leadsnebula_core::normalize_env_for_ssm(&environment);
    let ssm = Arc::new(SsmService::new(environment.clone(), None).await?);
    let config_path = format!("/leadsnebula/{}/rust/", env_normalized);
    let params = ssm.get_parameters_by_path(&config_path).await?;

    let database_url = params
        .get(&format!(
            "/leadsnebula/{}/rust/db/connection_url",
            env_normalized
        ))
        .cloned()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "DATABASE_URL not found in SSM at /leadsnebula/{}/rust/db/connection_url",
                env_normalized
            )
        })?;

    let pool = create_pool(&database_url).await?;

    println!("Applying migration: create_publisher_verticals");

    // Read and execute the migration SQL
    let migration_sql =
        fs::read_to_string("migrations/20250107000004_create_publisher_verticals.sql")?;

    sqlx::raw_sql(&migration_sql).execute(&pool).await?;

    println!("✅ Migration applied successfully");

    // Record the migration in _sqlx_migrations table (if it exists)
    let _ =
        sqlx::query("INSERT INTO _sqlx_migrations (version) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(20250107000004i64)
            .execute(&pool)
            .await;

    println!("✅ Migration applied and recorded");

    Ok(())
}
