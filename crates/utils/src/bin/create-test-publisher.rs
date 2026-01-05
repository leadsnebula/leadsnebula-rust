use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env.local
    let _ = dotenv::from_filename(".env.local");
    let _ = dotenv::dotenv();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = create_pool(&database_url).await?;

    // Generate hashes
    let api_key = "pk_live_c2f93c1c894585c46cbfb1f34c3020b8";
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let api_key_hash = hex::encode(hasher.finalize());

    let hmac_secret = "0811b4aa3c8a3b826b4d73ab90a8a0f05317c2b1c94601606ee275bb932235f4e98a823c357ff9c253a8de124f5e1bb850d6cd9258500b161f12b721d4933de1";
    let mut hasher = Sha256::new();
    hasher.update(hmac_secret.as_bytes());
    let hmac_secret_hash = hex::encode(hasher.finalize());

    println!("API Key Hash: {}", api_key_hash);
    println!("HMAC Secret Hash: {}\n", hmac_secret_hash);

    // Check if instances table exists
    let instances_table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'instances')",
    )
    .fetch_one(&pool)
    .await?;

    let instance_id = if instances_table_exists {
        let instance_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM instances LIMIT 1")
            .fetch_optional(&pool)
            .await?;

        match instance_id {
            Some(id) => id,
            None => {
                println!("⚠️  No instances found. Creating a default instance...");
                // Check if instance_users table exists for foreign key
                let users_exist: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'instance_users')"
                )
                .fetch_one(&pool)
                .await?;

                let new_instance_id = Uuid::new_v4();
                if users_exist {
                    // Try to get a user for the foreign key
                    let user_id: Option<Uuid> =
                        sqlx::query_scalar("SELECT id FROM instance_users LIMIT 1")
                            .fetch_optional(&pool)
                            .await?;

                    if let Some(uid) = user_id {
                        sqlx::query("INSERT INTO instances (id, name, instance_user_id, payment_status, created_at, updated_at) VALUES ($1, 'Default Instance', $2, 'trial', NOW(), NOW())")
                            .bind(new_instance_id)
                            .bind(uid)
                            .execute(&pool)
                            .await?;
                    } else {
                        println!(
                            "⚠️  No instance_users found. Creating instance without user_id..."
                        );
                        sqlx::query("INSERT INTO instances (id, name, payment_status, created_at, updated_at) VALUES ($1, 'Default Instance', 'trial', NOW(), NOW())")
                            .bind(new_instance_id)
                            .execute(&pool)
                            .await?;
                    }
                } else {
                    sqlx::query("INSERT INTO instances (id, name, payment_status, created_at, updated_at) VALUES ($1, 'Default Instance', 'trial', NOW(), NOW())")
                        .bind(new_instance_id)
                        .execute(&pool)
                        .await?;
                }
                new_instance_id
            }
        }
    } else {
        println!("⚠️  Instances table does not exist. Using a placeholder UUID.");
        println!("⚠️  Note: Publisher creation may fail if instance_id is required.");
        Uuid::new_v4() // Placeholder
    };

    // Check if publisher already exists
    let existing: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM publishers WHERE api_key_hash = $1")
            .bind(&api_key_hash)
            .fetch_optional(&pool)
            .await?;

    if let Some(pub_id) = existing {
        println!("✓ Publisher already exists with ID: {}", pub_id);

        // Update HMAC if needed
        let hmac_exists: Option<String> =
            sqlx::query_scalar("SELECT hmac_secret_hash FROM publishers WHERE id = $1")
                .bind(pub_id)
                .fetch_optional(&pool)
                .await?;

        if hmac_exists.is_none() || hmac_exists.as_deref() != Some(&hmac_secret_hash) {
            println!("Updating HMAC secret...");
            sqlx::query(
                "UPDATE publishers SET hmac_secret_hash = $1, hmac_secret_prefix = $2, hmac_required = true WHERE id = $3"
            )
            .bind(&hmac_secret_hash)
            .bind("hmac_live_")
            .bind(pub_id)
            .execute(&pool)
            .await?;
            println!("✓ HMAC secret updated");
        } else {
            println!("✓ HMAC secret already set");
        }
    } else {
        println!("Creating new publisher...");
        let pub_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO publishers (
                id, name, email, api_key_hash, api_key_prefix, status,
                instance_id, is_documentation_test, hmac_secret_hash, hmac_secret_prefix, hmac_required,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW(), NOW()
            )
            "#
        )
        .bind(pub_id)
        .bind("Only Solar Test")
        .bind("test@onlysolar.com")
        .bind(&api_key_hash)
        .bind("pk_live_")
        .bind("active")
        .bind(instance_id)
        .bind(false)
        .bind(&hmac_secret_hash)
        .bind("hmac_live_")
        .bind(true)
        .execute(&pool)
        .await?;
        println!("✓ Publisher created with ID: {}", pub_id);
    }

    // Get publisher ID
    let pub_id: Uuid = sqlx::query_scalar("SELECT id FROM publishers WHERE api_key_hash = $1")
        .bind(&api_key_hash)
        .fetch_one(&pool)
        .await?;

    println!("\n=== Publisher Information ===");
    println!("Publisher ID: {}", pub_id);
    println!("API Key: {}", api_key);
    println!("API Key Hash: {}", api_key_hash);
    println!("HMAC Secret Hash: {}", hmac_secret_hash);

    // Check for campaign - first by publisher, then search all campaigns
    let campaign: Option<(Uuid, String)> = sqlx::query(
        "SELECT id, campaign_token FROM campaigns WHERE publisher_id = $1 AND (name ILIKE '%only solar%' OR name ILIKE '%solar%test%') LIMIT 1"
    )
    .bind(pub_id)
    .map(|row: sqlx::postgres::PgRow| {
        (row.get::<Uuid, _>(0), row.get::<String, _>(1))
    })
    .fetch_optional(&pool)
    .await?;

    if let Some((camp_id, token)) = campaign {
        println!("\n=== Campaign Information ===");
        println!("Campaign ID: {}", camp_id);
        println!("Campaign Token: {}", token);
    } else {
        // Search all campaigns for "only solar"
        println!("\nSearching all campaigns for 'only solar'...");
        let all_campaigns: Vec<(Uuid, String, Option<String>, Option<Uuid>)> = sqlx::query(
            "SELECT id, campaign_token, name, publisher_id FROM campaigns WHERE name ILIKE '%only solar%' OR name ILIKE '%solar%test%' OR campaign_token ILIKE '%only%' LIMIT 10"
        )
        .map(|row: sqlx::postgres::PgRow| {
            (row.get(0), row.get(1), row.get(2), row.get(3))
        })
        .fetch_all(&pool)
        .await?;

        if !all_campaigns.is_empty() {
            println!("Found {} campaign(s):", all_campaigns.len());
            for (camp_id, token, name, pub_id_opt) in all_campaigns {
                println!("  Campaign ID: {}", camp_id);
                println!("  Campaign Token: {}", token);
                println!("  Name: {}", name.unwrap_or_else(|| "NULL".to_string()));
                println!(
                    "  Publisher ID: {}",
                    pub_id_opt
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "NULL".to_string())
                );
                println!("  ---");
            }
        } else {
            println!("⚠️  No campaign found with 'only solar' in name");
            println!("Listing all campaigns:");
            let all: Vec<(Uuid, String, Option<String>, Option<Uuid>)> = sqlx::query(
                "SELECT id, campaign_token, name, publisher_id FROM campaigns LIMIT 10",
            )
            .map(|row: sqlx::postgres::PgRow| (row.get(0), row.get(1), row.get(2), row.get(3)))
            .fetch_all(&pool)
            .await?;

            if all.is_empty() {
                println!("  No campaigns in database");
            } else {
                for (camp_id, token, name, pub_id_opt) in all {
                    println!("  Campaign ID: {}", camp_id);
                    println!("  Campaign Token: {}", token);
                    println!("  Name: {}", name.unwrap_or_else(|| "NULL".to_string()));
                    println!(
                        "  Publisher ID: {}",
                        pub_id_opt
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "NULL".to_string())
                    );
                    println!("  ---");
                }
            }
        }
    }

    Ok(())
}
