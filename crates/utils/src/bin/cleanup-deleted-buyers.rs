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

    // First, find all buyers with deleted_at set
    println!("Finding buyers with deleted_at set...");
    #[derive(sqlx::FromRow)]
    struct BuyerRow {
        id: uuid::Uuid,
        name: String,
        instance_id: uuid::Uuid,
        deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    let deleted_buyers = sqlx::query_as::<_, BuyerRow>(
        "SELECT id, name, instance_id, deleted_at FROM buyers WHERE deleted_at IS NOT NULL",
    )
    .fetch_all(&pool)
    .await?;

    if deleted_buyers.is_empty() {
        println!("No deleted buyers found.");
        return Ok(());
    }

    println!("Found {} deleted buyer(s):", deleted_buyers.len());
    for buyer in &deleted_buyers {
        println!(
            "  - ID: {}, Name: {}, Instance: {}, Deleted at: {:?}",
            buyer.id, buyer.name, buyer.instance_id, buyer.deleted_at
        );
    }

    // Delete associated campaigns first
    for buyer in &deleted_buyers {
        println!(
            "Deleting campaigns for buyer {} ({})...",
            buyer.id, buyer.name
        );
        let result = sqlx::query("DELETE FROM campaigns WHERE buyer_id = $1")
            .bind(buyer.id)
            .execute(&pool)
            .await?;
        println!("  Deleted {} campaign(s)", result.rows_affected());
    }

    // Now delete the buyers
    println!("\nDeleting buyers...");
    for buyer in &deleted_buyers {
        println!("Deleting buyer {} ({})...", buyer.id, buyer.name);
        let result = sqlx::query("DELETE FROM buyers WHERE id = $1")
            .bind(buyer.id)
            .execute(&pool)
            .await?;
        println!("  Deleted {} buyer record(s)", result.rows_affected());
    }

    println!("\nCleanup complete!");
    Ok(())
}
