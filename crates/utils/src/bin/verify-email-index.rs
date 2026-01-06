use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use leadsnebula_core::ssm::SsmService;
use sqlx::Row;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development
    let _ = dotenv::from_filename(".env.local");
    let _ = dotenv::dotenv();

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

    println!("Checking publishers table email constraints and indexes...\n");

    // Check for unique constraint on email
    let constraints = sqlx::query(
        "SELECT conname FROM pg_constraint WHERE conrelid = 'publishers'::regclass AND contype = 'u' AND conkey::text LIKE '%email%'"
    )
    .fetch_all(&pool)
    .await?;

    println!("Unique constraints on email:");
    if constraints.is_empty() {
        println!("  ✅ No unique constraint on email (expected - should be removed)");
    } else {
        for row in constraints {
            println!("  - {}", row.get::<String, _>("conname"));
        }
    }

    // Check for partial unique index
    let indexes = sqlx::query(
        "SELECT indexname, indexdef FROM pg_indexes WHERE tablename = 'publishers' AND indexname LIKE '%email%'"
    )
    .fetch_all(&pool)
    .await?;

    println!("\nEmail-related indexes:");
    if indexes.is_empty() {
        println!("  ⚠️  No email indexes found");
    } else {
        for row in indexes {
            let name: String = row.get("indexname");
            let def: String = row.get("indexdef");
            println!("  - {}: {}", name, def);
            if name.contains("email_unique_not_deleted") {
                println!(
                    "    ✅ Partial unique index found (allows deleted publishers to reuse emails)"
                );
            }
        }
    }

    Ok(())
}
