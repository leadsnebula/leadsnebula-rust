use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env.local
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = create_pool(&database_url).await?;

    println!("=== Creating Test Data ===\n");

    // 1. Get or create instance
    let instance_id = get_or_create_instance(&pool).await?;
    println!("✓ Instance ID: {}\n", instance_id);

    // 2. Get or create publisher
    let publisher_id = get_or_create_publisher(&pool, &instance_id).await?;
    println!("✓ Publisher ID: {}\n", publisher_id);

    // 3. Get or create buyer
    let buyer_id = get_or_create_buyer(&pool, &instance_id).await?;
    println!("✓ Buyer ID: {}\n", buyer_id);

    // 4. Get or create campaign
    let campaign_id = get_or_create_campaign(&pool, &instance_id, &publisher_id, &buyer_id).await?;
    println!("✓ Campaign ID: {}\n", campaign_id);

    // 5. Get or create ping tree
    let ping_tree_id = get_or_create_ping_tree(&pool, &instance_id, &publisher_id).await?;
    println!("✓ Ping Tree ID: {}\n", ping_tree_id);

    // 6. Link campaign to ping tree
    link_campaign_to_ping_tree(&pool, &ping_tree_id, &campaign_id).await?;
    println!("✓ Linked campaign to ping tree\n");

    println!("=== Test Data Summary ===");
    println!("Instance ID: {}", instance_id);
    println!("Publisher ID: {}", publisher_id);
    println!("Buyer ID: {}", buyer_id);
    println!("Campaign ID: {}", campaign_id);
    println!("Ping Tree ID: {}", ping_tree_id);
    println!("\n✅ All test data created successfully!");

    Ok(())
}

async fn get_or_create_instance(pool: &sqlx::PgPool) -> Result<Uuid> {
    let instance_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM instances LIMIT 1")
        .fetch_optional(pool)
        .await?;

    if let Some(id) = instance_id {
        return Ok(id);
    }

    let new_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO instances (id, name, payment_status, created_at, updated_at) VALUES ($1, 'Test Instance', 'trial', NOW(), NOW())"
    )
    .bind(new_id)
    .execute(pool)
    .await?;

    Ok(new_id)
}

async fn get_or_create_publisher(pool: &sqlx::PgPool, instance_id: &Uuid) -> Result<Uuid> {
    let api_key = "pk_test_1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let api_key_hash = hex::encode(hasher.finalize());

    let publisher_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM publishers WHERE api_key_hash = $1")
            .bind(&api_key_hash)
            .fetch_optional(pool)
            .await?;

    if let Some(id) = publisher_id {
        return Ok(id);
    }

    let new_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO publishers (
            id, name, email, api_key_hash, api_key_prefix, status,
            instance_id, is_documentation_test, created_at, updated_at
        ) VALUES (
            $1, 'Test Publisher', 'test@example.com', $2, 'pk_test_', 'active',
            $3, false, NOW(), NOW()
        )
        "#,
    )
    .bind(new_id)
    .bind(&api_key_hash)
    .bind(instance_id)
    .execute(pool)
    .await?;

    println!("Created publisher with API key: {}", api_key);
    Ok(new_id)
}

async fn get_or_create_buyer(pool: &sqlx::PgPool, instance_id: &Uuid) -> Result<Uuid> {
    let buyer_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM buyers WHERE name = 'Test Buyer' AND instance_id = $1 LIMIT 1",
    )
    .bind(instance_id)
    .fetch_optional(pool)
    .await?;

    if let Some(id) = buyer_id {
        return Ok(id);
    }

    let new_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO buyers (
            id, name, instance_id, status, created_at, updated_at
        ) VALUES (
            $1, 'Test Buyer', $2, 'active', NOW(), NOW()
        )
        "#,
    )
    .bind(new_id)
    .bind(instance_id)
    .execute(pool)
    .await?;

    Ok(new_id)
}

async fn get_or_create_campaign(
    pool: &sqlx::PgPool,
    instance_id: &Uuid,
    publisher_id: &Uuid,
    buyer_id: &Uuid,
) -> Result<Uuid> {
    let campaign_token = "test_campaign_token_123";
    let campaign_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM campaigns WHERE campaign_token = $1 LIMIT 1")
            .bind(campaign_token)
            .fetch_optional(pool)
            .await?;

    if let Some(id) = campaign_id {
        return Ok(id);
    }

    let new_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO campaigns (
            id, buyer_id, publisher_id, instance_id, name, vertical,
            campaign_token, status, is_documentation_test, created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, 'Test Campaign', 'solar',
            $5, 'active', false, NOW(), NOW()
        )
        "#,
    )
    .bind(new_id)
    .bind(buyer_id)
    .bind(publisher_id)
    .bind(instance_id)
    .bind(campaign_token)
    .execute(pool)
    .await?;

    println!("Created campaign with token: {}", campaign_token);
    Ok(new_id)
}

async fn get_or_create_ping_tree(
    pool: &sqlx::PgPool,
    instance_id: &Uuid,
    publisher_id: &Uuid,
) -> Result<Uuid> {
    let ping_tree_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM ping_trees WHERE publisher_id = $1 AND vertical = 'solar' LIMIT 1",
    )
    .bind(publisher_id)
    .fetch_optional(pool)
    .await?;

    if let Some(id) = ping_tree_id {
        return Ok(id);
    }

    let new_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO ping_trees (
            id, instance_id, publisher_id, name, vertical, strategy, status,
            priority, created_at, updated_at
        ) VALUES (
            $1, $2, $3, 'Test Ping Tree', 'solar', 'ping_post', 'active',
            1, NOW(), NOW()
        )
        "#,
    )
    .bind(new_id)
    .bind(instance_id)
    .bind(publisher_id)
    .execute(pool)
    .await?;

    Ok(new_id)
}

async fn link_campaign_to_ping_tree(
    pool: &sqlx::PgPool,
    ping_tree_id: &Uuid,
    campaign_id: &Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ping_tree_campaigns WHERE ping_tree_id = $1 AND campaign_id = $2)"
    )
    .bind(ping_tree_id)
    .bind(campaign_id)
    .fetch_one(pool)
    .await?;

    if exists {
        return Ok(());
    }

    sqlx::query(
        r#"
        INSERT INTO ping_tree_campaigns (
            id, ping_tree_id, campaign_id, priority, enabled, created_at, updated_at
        ) VALUES (
            gen_random_uuid(), $1, $2, 1, true, NOW(), NOW()
        )
        "#,
    )
    .bind(ping_tree_id)
    .bind(campaign_id)
    .execute(pool)
    .await?;

    Ok(())
}
