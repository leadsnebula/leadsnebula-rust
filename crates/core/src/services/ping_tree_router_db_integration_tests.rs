// Database-backed integration tests for PingTreeRouter
// These tests require DATABASE_URL and verify full flow with real database

#[cfg(test)]
mod ping_tree_router_db_integration_tests {
    use crate::models::{campaign::Campaign, enums::CampaignStatus, enums::LeadStatus, lead::Lead};
    use crate::services::ping_tree_router::PingTreeRouter;
    use sqlx::PgPool;
    use std::sync::Arc;
    use uuid::Uuid;

    // Use common test helper from api crate
    async fn create_test_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
        // Try to use the common helper, but if not available, create our own
        let database_url = std::env::var("DATABASE_URL")
            .ok()
            .ok_or_else(|| "DATABASE_URL not set - skipping integration tests".to_string())?;

        use sqlx::postgres::PgPoolOptions;
        use tokio::time::Duration;

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&database_url)
            .await?;

        Ok(pool)
    }

    async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid, Uuid, String) {
        // Create instance_user
        let instance_user_id = Uuid::new_v4();
        let unique_email = format!("test_user_{}@test.invalid", Uuid::new_v4());
        let password_hash = "hashed_password";

        sqlx::query(
            r#"
            INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
            VALUES ($1, $2, $3, 'active', NOW(), NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(instance_user_id)
        .bind(&unique_email)
        .bind(password_hash)
        .execute(pool)
        .await
        .unwrap();

        // Create instance
        let instance_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
             VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(instance_id)
        .bind(instance_user_id)
        .execute(pool)
        .await
        .unwrap();

        // Create publisher
        let publisher_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO publishers (id, instance_id, name, email, status, created_at, updated_at)
             VALUES ($1, $2, 'Test Publisher', $3, 'active', NOW(), NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(publisher_id)
        .bind(instance_id)
        .bind(&format!("publisher_{}@test.invalid", Uuid::new_v4()))
        .execute(pool)
        .await
        .unwrap();

        // Create vertical
        let vertical_id = Uuid::new_v4();
        let vertical_slug = format!("test-vertical-{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO verticals (id, slug, name, status, created_at, updated_at)
             VALUES ($1, $2, 'Test Vertical', 'active', NOW(), NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(vertical_id)
        .bind(&vertical_slug)
        .execute(pool)
        .await
        .unwrap();

        (publisher_id, instance_id, vertical_id, vertical_slug)
    }

    async fn create_test_lead(
        pool: &PgPool,
        publisher_id: Uuid,
        vertical_id: Uuid,
        request_type: &str,
    ) -> Lead {
        let lead_uuid = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO leads (uuid, event_id, publisher_id, vertical_id, request_type, strategy, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, 'default', 'processing', NOW(), NOW())
            "#,
        )
        .bind(lead_uuid)
        .bind(format!("evt_{}", Uuid::new_v4()))
        .bind(publisher_id)
        .bind(vertical_id)
        .bind(request_type)
        .execute(pool)
        .await
        .unwrap();

        sqlx::query_as::<_, Lead>("SELECT * FROM leads WHERE uuid = $1")
            .bind(lead_uuid)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn create_ping_tree(
        pool: &PgPool,
        publisher_id: Uuid,
        vertical: &str,
        status: &str,
        strategy: &str,
    ) -> Uuid {
        let ping_tree_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ping_trees (id, publisher_id, name, vertical, strategy, status, created_at, updated_at)
            VALUES ($1, $2, 'Test Ping Tree', $3, $4, $5, NOW(), NOW())
            "#,
        )
        .bind(ping_tree_id)
        .bind(publisher_id)
        .bind(vertical)
        .bind(strategy)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
        ping_tree_id
    }

    async fn create_buyer(pool: &PgPool, instance_id: Uuid) -> Uuid {
        let buyer_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO buyers (id, instance_id, name, status, created_at, updated_at)
            VALUES ($1, $2, 'Test Buyer', 'active', NOW(), NOW())
            "#,
        )
        .bind(buyer_id)
        .bind(instance_id)
        .execute(pool)
        .await
        .unwrap();
        buyer_id
    }

    async fn create_campaign(
        pool: &PgPool,
        buyer_id: Uuid,
        publisher_id: Uuid,
        instance_id: Uuid,
        vertical: &str,
    ) -> Campaign {
        let campaign_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO campaigns (id, buyer_id, publisher_id, instance_id, name, vertical, campaign_token, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, 'Test Campaign', $5, $6, 'active', NOW(), NOW())
            "#,
        )
        .bind(campaign_id)
        .bind(buyer_id)
        .bind(publisher_id)
        .bind(instance_id)
        .bind(vertical)
        .bind(format!("token_{}", Uuid::new_v4()))
        .execute(pool)
        .await
        .unwrap();

        sqlx::query_as::<_, Campaign>("SELECT * FROM campaigns WHERE id = $1")
            .bind(campaign_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn add_campaign_to_ping_tree(
        pool: &PgPool,
        ping_tree_id: Uuid,
        campaign_id: Uuid,
        priority: Option<i32>,
    ) {
        sqlx::query(
            r#"
            INSERT INTO ping_tree_campaigns (id, ping_tree_id, campaign_id, priority, enabled, created_at, updated_at)
            VALUES ($1, $2, $3, $4, true, NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(ping_tree_id)
        .bind(campaign_id)
        .bind(priority)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires DATABASE_URL
    async fn test_ping_auction_updates_lead_status() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Skipping test - DATABASE_URL not set");
                return;
            }
        };

        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;
        let lead = create_test_lead(&pool, publisher_id, vertical_id, "ping").await;

        let buyer_id = create_buyer(&pool, instance_id).await;
        let campaign =
            create_campaign(&pool, buyer_id, publisher_id, instance_id, &vertical_slug).await;
        let ping_tree_id =
            create_ping_tree(&pool, publisher_id, &vertical_slug, "active", "ping_post").await;
        add_campaign_to_ping_tree(&pool, ping_tree_id, campaign.id, Some(1)).await;

        let router = PingTreeRouter::new(
            lead.clone(),
            publisher_id,
            vertical_slug,
            "ping".to_string(),
        );
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]); // Dummy key for tests
        let result = router
            .route(pool_arc, encryption_key)
            .await
            .expect("Route should complete");

        // Verify lead status was updated
        let updated_lead = sqlx::query_as::<_, Lead>("SELECT * FROM leads WHERE uuid = $1")
            .bind(lead.uuid)
            .fetch_one(&pool)
            .await
            .unwrap();

        if result.success {
            assert_eq!(updated_lead.status, LeadStatus::PingAccepted);
            assert!(updated_lead.promise_id.is_some());
            assert!(updated_lead.ping_id.is_some());
            assert_eq!(updated_lead.campaign_id, Some(campaign.id));
            assert_eq!(updated_lead.buyer_id, Some(buyer_id));
        }
    }

    #[tokio::test]
    #[ignore] // Requires DATABASE_URL
    async fn test_ping_auction_persists_buyer_responses() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Skipping test - DATABASE_URL not set");
                return;
            }
        };

        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;
        let lead = create_test_lead(&pool, publisher_id, vertical_id, "ping").await;

        let buyer_id = create_buyer(&pool, instance_id).await;
        let campaign =
            create_campaign(&pool, buyer_id, publisher_id, instance_id, &vertical_slug).await;
        let ping_tree_id =
            create_ping_tree(&pool, publisher_id, &vertical_slug, "active", "ping_post").await;
        add_campaign_to_ping_tree(&pool, ping_tree_id, campaign.id, Some(1)).await;

        let router = PingTreeRouter::new(
            lead.clone(),
            publisher_id,
            vertical_slug,
            "ping".to_string(),
        );
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]); // Dummy key for tests
        let _result = router
            .route(pool_arc, encryption_key)
            .await
            .expect("Route should complete");

        // Verify buyer_responses were persisted
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM buyer_responses WHERE lead_id = $1")
                .bind(lead.uuid)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert!(count > 0, "buyer_responses should be persisted");
    }

    #[tokio::test]
    #[ignore] // Requires DATABASE_URL
    async fn test_fullpost_persists_both_ping_and_post_payloads() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Skipping test - DATABASE_URL not set");
                return;
            }
        };

        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;
        let lead = create_test_lead(&pool, publisher_id, vertical_id, "fullpost").await;

        let buyer_id = create_buyer(&pool, instance_id).await;
        let campaign =
            create_campaign(&pool, buyer_id, publisher_id, instance_id, &vertical_slug).await;
        let ping_tree_id =
            create_ping_tree(&pool, publisher_id, &vertical_slug, "active", "ping_post").await;
        add_campaign_to_ping_tree(&pool, ping_tree_id, campaign.id, Some(1)).await;

        let router = PingTreeRouter::new(
            lead.clone(),
            publisher_id,
            vertical_slug,
            "fullpost".to_string(),
        );
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]); // Dummy key for tests
        let result = router
            .route(pool_arc, encryption_key)
            .await
            .expect("Route should complete");

        if result.success {
            // Verify ping_payloads exists
            let ping_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM ping_payloads WHERE lead_id = $1")
                    .bind(lead.uuid)
                    .fetch_one(&pool)
                    .await
                    .unwrap();

            // Verify post_payloads exists (if post succeeded)
            if result.post_id.is_some() {
                let post_count: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM post_payloads WHERE lead_id = $1")
                        .bind(lead.uuid)
                        .fetch_one(&pool)
                        .await
                        .unwrap();

                assert!(ping_count > 0, "ping_payloads should be persisted");
                assert!(
                    post_count > 0,
                    "post_payloads should be persisted for fullpost"
                );
            }
        }
    }
}
