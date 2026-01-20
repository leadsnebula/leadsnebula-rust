use anyhow::Result;
use leadsnebula_core::ssm::SsmService;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;

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

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&database_url)
        .await?;

    println!("Removing migration records for modified migrations...");

    // Remove records for migrations that were modified during recent refactoring
    // These migrations were changed after being applied (schema changes, etc.)
    let versions_to_remove = vec![
        20250101000007i64, // create_ping_trees (publisher_id removed)
        20250116000001i64, // add_schema_comments (publisher_id comment removed)
        20260120000001i64, // fix_ping_tree_unique_constraint (likely modified)
        20260120000002i64, // create_ping_tree_publishers (IF NOT EXISTS added)
        20260120000003i64, // migrate_publisher_id_to_join_table (made conditional)
    ];

    let mut total_deleted = 0;
    for version in versions_to_remove {
        let result = sqlx::query("DELETE FROM _sqlx_migrations WHERE version = $1")
            .bind(version)
            .execute(&pool)
            .await?;
        let deleted = result.rows_affected();
        if deleted > 0 {
            println!("✅ Deleted migration record for {}", version);
            total_deleted += deleted;
        }
    }

    println!(
        "✅ Cleanup complete. Deleted {} migration record(s). You can now run migrations again.",
        total_deleted
    );

    Ok(())
}
