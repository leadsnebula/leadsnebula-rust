use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development (highest priority)
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
    let mut params = ssm.get_parameters_by_path(&config_path).await?;

    // For dev environment, also fetch from prod path as fallback
    if env_normalized == "dev" {
        let prod_params = ssm
            .get_parameters_by_path("/leadsnebula/prod/rust/")
            .await?;
        params.extend(prod_params);
    }

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

    println!("Connecting to database...");
    let pool = create_pool(&database_url).await?;

    // Find Solar vertical
    println!("Finding Solar vertical...");
    use sqlx::Row;
    let solar_vertical = sqlx::query("SELECT id FROM verticals WHERE slug = 'solar' LIMIT 1")
        .fetch_optional(&pool)
        .await?;

    let solar_vertical_id = match solar_vertical {
        Some(row) => {
            let id: uuid::Uuid = row.try_get("id")?;
            println!("Found Solar vertical: {}", id);
            id
        }
        None => {
            println!("Solar vertical not found. Creating it...");
            let new_id = uuid::Uuid::new_v4();
            sqlx::query("INSERT INTO verticals (id, name, slug, is_active, created_at, updated_at) VALUES ($1, 'Solar', 'solar', true, NOW(), NOW())")
                .bind(new_id)
                .execute(&pool)
                .await?;
            println!("Created Solar vertical: {}", new_id);
            new_id
        }
    };

    // Update all buyers without vertical_id to use Solar
    println!("\nUpdating buyers without vertical_id to use Solar...");
    let result = sqlx::query(
        "UPDATE buyers SET vertical_id = $1 WHERE vertical_id IS NULL AND deleted_at IS NULL",
    )
    .bind(solar_vertical_id)
    .execute(&pool)
    .await?;

    println!(
        "Updated {} buyer(s) with Solar vertical",
        result.rows_affected()
    );

    println!("\nDone!");
    Ok(())
}
