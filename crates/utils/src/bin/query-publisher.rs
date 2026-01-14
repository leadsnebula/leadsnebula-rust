use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use sha2::{Digest, Sha256};
use sqlx::Row;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env.local
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = create_pool(&database_url).await?;

    // Check if tables exist
    println!("=== Checking if tables exist ===");
    let table_check = sqlx::query(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'publishers') as publishers_exists, EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'campaigns') as campaigns_exists"
    )
    .fetch_one(&pool)
    .await?;

    println!(
        "Publishers table exists: {}",
        table_check.get::<bool, _>("publishers_exists")
    );
    println!(
        "Campaigns table exists: {}",
        table_check.get::<bool, _>("campaigns_exists")
    );

    if !table_check.get::<bool, _>("publishers_exists") {
        println!("⚠️  Publishers table does not exist. Run migrations first!");
        return Ok(());
    }

    // Count records
    let pub_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publishers")
        .fetch_one(&pool)
        .await?;
    println!("Total publishers in database: {}", pub_count);

    let camp_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM campaigns")
        .fetch_one(&pool)
        .await?;
    println!("Total campaigns in database: {}\n", camp_count);

    // Query for "only solar" publisher
    println!("=== Searching for 'Only Solar' Publisher ===");
    let rows = sqlx::query(
        "SELECT id, name, email, api_key_hash, api_key_prefix, status FROM publishers WHERE name ILIKE '%only solar%' OR name ILIKE '%solar%test%' OR name ILIKE '%only%'"
    )
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        println!("No publishers found with 'only solar' in name. Listing all publishers:");
        let all_rows = sqlx::query(
            "SELECT id, name, email, api_key_hash, api_key_prefix, status FROM publishers LIMIT 10",
        )
        .fetch_all(&pool)
        .await?;

        for row in all_rows {
            println!("Publisher ID: {}", row.get::<uuid::Uuid, _>("id"));
            println!("Name: {}", row.get::<String, _>("name"));
            println!("Email: {}", row.get::<String, _>("email"));
            println!("API Key Prefix: {}", row.get::<String, _>("api_key_prefix"));
            println!("Status: {}", row.get::<String, _>("status"));
            println!("---");
        }
    }

    for row in rows {
        println!("Publisher ID: {}", row.get::<uuid::Uuid, _>("id"));
        println!("Name: {}", row.get::<String, _>("name"));
        println!("Email: {}", row.get::<String, _>("email"));
        println!("API Key Prefix: {}", row.get::<String, _>("api_key_prefix"));
        println!("Status: {}", row.get::<String, _>("status"));
        println!("API Key Hash: {}", row.get::<String, _>("api_key_hash"));
        println!("---");
    }

    // Check API key
    println!("\n=== Checking API Key: pk_live_c2f93c1c894585c46cbfb1f34c3020b8 ===");
    let api_key = "pk_live_c2f93c1c894585c46cbfb1f34c3020b8";
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let api_key_hash = hex::encode(hasher.finalize());
    println!("API Key Hash: {}", api_key_hash);

    let row = sqlx::query(
        "SELECT id, name, email, api_key_hash, api_key_prefix, status FROM publishers WHERE api_key_hash = $1"
    )
    .bind(&api_key_hash)
    .fetch_optional(&pool)
    .await?;

    if let Some(row) = row {
        println!("✓ API Key FOUND in database");
        println!("Publisher ID: {}", row.get::<uuid::Uuid, _>("id"));
        println!("Name: {}", row.get::<String, _>("name"));
        println!("Email: {}", row.get::<String, _>("email"));
    } else {
        println!("✗ API Key NOT FOUND in database");
    }

    // Check HMAC
    println!("\n=== Checking HMAC Secret ===");
    let hmac_secret = "0811b4aa3c8a3b826b4d73ab90a8a0f05317c2b1c94601606ee275bb932235f4e98a823c357ff9c253a8de124f5e1bb850d6cd9258500b161f12b721d4933de1";
    let mut hasher = Sha256::new();
    hasher.update(hmac_secret.as_bytes());
    let hmac_hash = hex::encode(hasher.finalize());
    println!("HMAC Secret Hash: {}", hmac_hash);

    let row = sqlx::query(
        "SELECT id, name, email, hmac_secret_hash, hmac_secret_prefix, hmac_required FROM publishers WHERE hmac_secret_hash = $1"
    )
    .bind(&hmac_hash)
    .fetch_optional(&pool)
    .await?;

    if let Some(row) = row {
        println!("✓ HMAC Secret FOUND in database");
        println!("Publisher ID: {}", row.get::<uuid::Uuid, _>("id"));
        println!("Name: {}", row.get::<String, _>("name"));
        println!("HMAC Required: {}", row.get::<bool, _>("hmac_required"));
    } else {
        println!("✗ HMAC Secret NOT FOUND in database");
    }

    // Query for campaign
    println!("\n=== Searching for 'Only Solar' Campaign ===");
    let rows = sqlx::query(
        "SELECT c.id, c.name, c.campaign_token, c.status, p.name as publisher_name, p.id as publisher_id FROM campaigns c JOIN publishers p ON c.publisher_id = p.id WHERE c.name ILIKE '%only solar%' OR c.name ILIKE '%solar%test%' OR c.name ILIKE '%only%'"
    )
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        println!("No campaigns found with 'only solar' in name. Listing all campaigns:");
        let all_rows = sqlx::query(
            "SELECT c.id, c.name, c.campaign_token, c.status, p.name as publisher_name, p.id as publisher_id FROM campaigns c JOIN publishers p ON c.publisher_id = p.id LIMIT 10"
        )
        .fetch_all(&pool)
        .await?;

        for row in all_rows {
            println!("Campaign ID: {}", row.get::<uuid::Uuid, _>("id"));
            println!(
                "Name: {}",
                row.get::<Option<String>, _>("name")
                    .unwrap_or_else(|| "NULL".to_string())
            );
            println!("Campaign Token: {}", row.get::<String, _>("campaign_token"));
            println!("Status: {}", row.get::<String, _>("status"));
            println!(
                "Publisher: {} (ID: {})",
                row.get::<String, _>("publisher_name"),
                row.get::<uuid::Uuid, _>("publisher_id")
            );
            println!("---");
        }
    }

    for row in rows {
        println!("Campaign ID: {}", row.get::<uuid::Uuid, _>("id"));
        println!("Name: {}", row.get::<String, _>("name"));
        println!("Campaign Token: {}", row.get::<String, _>("campaign_token"));
        println!("Status: {}", row.get::<String, _>("status"));
        println!(
            "Publisher: {} (ID: {})",
            row.get::<String, _>("publisher_name"),
            row.get::<uuid::Uuid, _>("publisher_id")
        );
        println!("---");
    }

    Ok(())
}
