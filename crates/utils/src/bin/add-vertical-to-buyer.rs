use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development (highest priority)
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
            let id: Uuid = row.try_get("id")?;
            println!("Found Solar vertical: {}", id);
            id
        }
        None => {
            println!("Solar vertical not found. Creating it...");
            let new_id = Uuid::new_v4();
            sqlx::query("INSERT INTO verticals (id, name, slug, is_active, created_at, updated_at) VALUES ($1, 'Solar', 'solar', true, NOW(), NOW())")
                .bind(new_id)
                .execute(&pool)
                .await?;
            println!("Created Solar vertical: {}", new_id);
            new_id
        }
    };

    // Find buyer named "Solar 1"
    println!("\nFinding buyer 'Solar 1'...");
    let buyer = sqlx::query("SELECT id, name, vertical_id FROM buyers WHERE name = 'Solar 1' AND deleted_at IS NULL LIMIT 1")
        .fetch_optional(&pool)
        .await?;

    match buyer {
        Some(row) => {
            let buyer_id: Uuid = row.try_get("id")?;
            let buyer_name: String = row.try_get("name")?;
            let current_vertical: Option<Uuid> = row.try_get("vertical_id").ok();

            println!("Found buyer: {} ({})", buyer_name, buyer_id);
            if let Some(vid) = current_vertical {
                println!("  Current vertical_id: {}", vid);
            } else {
                println!("  No vertical_id set");
            }

            // Update buyer with Solar vertical
            println!("\nUpdating buyer with Solar vertical...");
            let result = sqlx::query("UPDATE buyers SET vertical_id = $1 WHERE id = $2")
                .bind(solar_vertical_id)
                .bind(buyer_id)
                .execute(&pool)
                .await?;

            println!("Updated {} buyer record(s)", result.rows_affected());
        }
        None => {
            println!("Buyer 'Solar 1' not found.");
        }
    }

    println!("\nDone!");
    Ok(())
}
