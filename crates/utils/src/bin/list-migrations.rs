use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::Row;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let env_loaded = dotenvy::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenvy::dotenv();
    }

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
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL not found"))?;

    let pool = create_pool(&database_url).await?;

    // Inspect columns present in _sqlx_migrations
    let cols_rows = sqlx::query("SELECT column_name FROM information_schema.columns WHERE table_name = '_sqlx_migrations' ORDER BY ordinal_position")
        .fetch_all(&pool)
        .await?;

    println!("_sqlx_migrations table columns:");
    for c in cols_rows {
        let name: String = c.try_get("column_name")?;
        println!("- {}", name);
    }

    // Fetch rows generically and print as text for known columns if present
    let rows = sqlx::query("SELECT * FROM _sqlx_migrations ORDER BY version DESC LIMIT 100")
        .fetch_all(&pool)
        .await?;

    println!("\nSample rows (latest first):");
    for r in rows {
        // Try common column names
        let version: Option<String> = r.try_get("version").ok().map(|v: i64| v.to_string());
        let installed_on: Option<String> = r
            .try_get::<Option<chrono::NaiveDateTime>, _>("installed_on")
            .ok()
            .flatten()
            .map(|d| d.to_string());
        let applied_on: Option<String> = r
            .try_get::<Option<chrono::NaiveDateTime>, _>("applied_on")
            .ok()
            .flatten()
            .map(|d| d.to_string());
        let checksum: Option<String> = r.try_get::<Option<String>, _>("checksum").ok().flatten();
        let description: Option<String> =
            r.try_get::<Option<String>, _>("description").ok().flatten();

        println!(
            "- version={:?} installed_on={:?} applied_on={:?} checksum={:?} description={:?}",
            version, installed_on, applied_on, checksum, description
        );
    }

    Ok(())
}
