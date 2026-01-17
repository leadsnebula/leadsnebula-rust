// Integration tests for PingTreeRouter
// These tests require a database connection

#[cfg(test)]
mod ping_tree_router_integration_tests {
    use crate::models::{
        campaign::Campaign,
        enums::{CampaignStatus, LeadStatus},
        lead::Lead,
        ping_tree::PingTree,
        ping_tree_campaign::PingTreeCampaign,
    };
    use crate::services::ping_tree_router::PingTreeRouter;
    use sqlx::PgPool;
    use std::sync::Arc;
    use uuid::Uuid;

    // Helper to create test data
    async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid, Uuid, String) {
        // Use a single transaction for all setup (1 connection instead of 4)
        let mut tx = pool.begin().await
            .expect("Failed to begin transaction for setup");

        // Create instance_user
        let instance_user_id = Uuid::new_v4();
        let unique_email = format!("test_user_{}@test.invalid", Uuid::new_v4());
        let password_hash = "hashed_password".to_string(); // Simplified for tests

        sqlx::query(
            r#"
            INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
            VALUES ($1, $2, $3, 'active', NOW(), NOW(), NOW())
            "#,
        )
        .bind(instance_user_id)
        .bind(&unique_email)
        .bind(&password_hash)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Create instance
        let instance_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
             VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())",
        )
        .bind(instance_id)
        .bind(instance_user_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Create publisher
        let publisher_id = Uuid::new_v4();
        let api_key_hash = format!("hash_{}", Uuid::new_v4());
        let api_key_prefix = format!("pk_{}", &api_key_hash[..8]);
        sqlx::query(
            "INSERT INTO publishers (id, instance_id, name, email, api_key_hash, api_key_prefix, api_key_encrypted, status, created_at, updated_at)
             VALUES ($1, $2, 'Test Publisher', $3, $4, $5, $6, 'active', NOW(), NOW())",
        )
        .bind(publisher_id)
        .bind(instance_id)
        .bind(&format!("publisher_{}@test.invalid", Uuid::new_v4()))
        .bind(&api_key_hash)
        .bind(&api_key_prefix)
        .bind("")
        .execute(&mut *tx)
        .await
        .unwrap();

        // Create vertical
        let vertical_id = Uuid::new_v4();
        let vertical_slug = format!("test-vertical-{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO verticals (id, slug, name, is_active, created_at, updated_at)
             VALUES ($1, $2, 'Test Vertical', true, NOW(), NOW())",
        )
        .bind(vertical_id)
        .bind(&vertical_slug)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Commit transaction (releases connection immediately)
        tx.commit().await.unwrap();

        (publisher_id, instance_id, vertical_id, vertical_slug)
    }

    async fn create_test_lead(
        pool: &PgPool,
        publisher_id: Uuid,
        vertical_id: Uuid,
        instance_id: Uuid,
        request_type: &str,
    ) -> Lead {
        let lead_uuid = Uuid::new_v4();
        // Ensure buyer and campaign exist to satisfy FK constraints
        let buyer_id = create_buyer(pool, instance_id).await;
        let campaign =
            create_campaign(pool, buyer_id, publisher_id, instance_id, "test-vertical").await;
        let session_id = format!("sess_{}", Uuid::new_v4());
        let post_id = format!("post_{}", Uuid::new_v4());
        // Sanitize request_type for DB constraints: DB only allows 'ping', 'post', 'fullpost'
        let db_request_type = match request_type {
            "ping" | "post" | "fullpost" => request_type.to_string(),
            _ => "post".to_string(),
        };
        let strategy_val = if db_request_type == "ping" {
            "pingPost".to_string()
        } else {
            "fullPost".to_string()
        };

        sqlx::query(
            r#"
            INSERT INTO leads (uuid, event_id, publisher_id, vertical_id, request_type, strategy, status, tcpa_consent, tcpa_language, submitted_at, buyer_id, campaign_id, post_id, session_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, 'processing', false, '', NOW(), $7, $8, $9, $10, NOW(), NOW())
            "#,
        )
        .bind(lead_uuid)
        .bind(format!("evt_{}", Uuid::new_v4()))
        .bind(publisher_id)
        .bind(vertical_id)
        .bind(&db_request_type)
        .bind(&strategy_val)
        .bind(buyer_id)
        .bind(campaign.id)
        .bind(&post_id)
        .bind(&session_id)
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
        instance_id: Uuid,
        vertical: &str,
        status: &str,
    ) -> Uuid {
        let ping_tree_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ping_trees (id, instance_id, publisher_id, name, vertical, strategy, status, created_at, updated_at)
            VALUES ($1, $2, $3, 'Test Ping Tree', $4, 'ping_post', $5, NOW(), NOW())
            "#,
        )
        .bind(ping_tree_id)
        .bind(instance_id)
        .bind(publisher_id)
        .bind(vertical)
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
    #[ignore] // Requires database setup
    async fn test_route_no_ping_tree() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => return, // Skip if DB not available
        };
        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;
        let lead = create_test_lead(&pool, publisher_id, vertical_id, instance_id, "ping").await;

        let router = PingTreeRouter::new(lead, publisher_id, vertical_slug, "ping".to_string());
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]); // Dummy key for tests
        let result = router
            .route(pool_arc, encryption_key)
            .await
            .expect("Route should complete");

        assert!(!result.success);
        assert_eq!(result.status, "error");
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("No active ping tree found"));
    }

    #[tokio::test]
    #[ignore] // Requires database setup
    async fn test_route_inactive_ping_tree() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => {
                // Skip test if database not available
                return;
            }
        };
        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;
        let lead = create_test_lead(&pool, publisher_id, vertical_id, instance_id, "ping").await;

        // Create inactive ping tree
        create_ping_tree(&pool, publisher_id, instance_id, &vertical_slug, "paused").await;

        let router = PingTreeRouter::new(lead, publisher_id, vertical_slug, "ping".to_string());
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]); // Dummy key for tests
        let result = router
            .route(pool_arc, encryption_key)
            .await
            .expect("Route should complete");

        assert!(!result.success);
        assert_eq!(result.status, "error");
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("paused"));
    }

    #[tokio::test]
    #[ignore] // Requires database setup
    async fn test_route_no_campaigns() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => return, // Skip if DB not available
        };
        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;
        let lead = create_test_lead(&pool, publisher_id, vertical_id, instance_id, "ping").await;

        // Create active ping tree but no campaigns
        create_ping_tree(&pool, publisher_id, instance_id, &vertical_slug, "active").await;

        let router = PingTreeRouter::new(lead, publisher_id, vertical_slug, "ping".to_string());
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]); // Dummy key for tests
        let result = router
            .route(pool_arc, encryption_key)
            .await
            .expect("Route should complete");

        assert!(!result.success);
        assert_eq!(result.status, "error");
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("No active campaigns"));
    }

    #[tokio::test]
    #[ignore] // Requires database setup
    async fn test_route_unknown_request_type() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => return, // Skip if DB not available
        };
        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;
        let lead = create_test_lead(&pool, publisher_id, vertical_id, instance_id, "unknown").await;

        let buyer_id = create_buyer(&pool, instance_id).await;
        let campaign =
            create_campaign(&pool, buyer_id, publisher_id, instance_id, &vertical_slug).await;
        let ping_tree_id =
            create_ping_tree(&pool, publisher_id, instance_id, &vertical_slug, "active").await;
        add_campaign_to_ping_tree(&pool, ping_tree_id, campaign.id, Some(1)).await;

        let router = PingTreeRouter::new(lead, publisher_id, vertical_slug, "unknown".to_string());
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]);
        let result = router
            .route(pool_arc, encryption_key)
            .await
            .expect("Route should complete");

        assert!(!result.success);
        assert_eq!(result.status, "error");
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("Unknown request_type"));
    }

    // Use unified test helper
    async fn create_test_pool() -> anyhow::Result<PgPool> {
        crate::test_helpers::create_test_pool().await
    }
}
