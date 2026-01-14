use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

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

    // Find buyer named "Solar 2"
    println!("Finding buyer 'Solar 2'...");
    let buyer = sqlx::query("SELECT id, name, buyer_type, post_type, vertical_id FROM buyers WHERE name = 'Solar 2' AND deleted_at IS NULL LIMIT 1")
        .fetch_optional(&pool)
        .await?;

    match buyer {
        Some(row) => {
            let buyer_id: Uuid = row.try_get("id")?;
            let buyer_name: String = row.try_get("name")?;
            let current_buyer_type: Option<String> = row.try_get("buyer_type").ok();
            let current_post_type: Option<String> = row.try_get("post_type").ok();
            let vertical_id: Option<Uuid> = row.try_get("vertical_id").ok();

            println!("Found buyer: {} ({})", buyer_name, buyer_id);
            println!("  Current buyer_type: {:?}", current_buyer_type);
            println!("  Current post_type: {:?}", current_post_type);
            println!("  Current vertical_id: {:?}", vertical_id);

            // Set buyer_type to "internal" and post_type to "ping_post"
            println!("\nUpdating buyer...");
            let result =
                sqlx::query("UPDATE buyers SET buyer_type = $1, post_type = $2 WHERE id = $3")
                    .bind("internal")
                    .bind("ping_post")
                    .bind(buyer_id)
                    .execute(&pool)
                    .await?;

            println!("Updated {} buyer record(s)", result.rows_affected());
            println!("  Set buyer_type to: internal");
            println!("  Set post_type to: ping_post");
        }
        None => {
            println!("Buyer 'Solar 2' not found.");
        }
    }

    println!("\nDone!");
    Ok(())
}
