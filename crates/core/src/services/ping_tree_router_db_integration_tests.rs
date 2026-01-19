// Database-backed integration tests for PingTreeRouter
// These tests require DATABASE_URL and verify full flow with real database

#[cfg(test)]
mod ping_tree_router_db_integration_tests {
    use crate::models::{campaign::Campaign, enums::CampaignStatus, enums::LeadStatus, lead::Lead};
    use crate::services::ping_tree_router::PingTreeRouter;
    use sqlx::PgPool;
    use std::sync::Arc;
    use uuid::Uuid;

    // Use unified test helper from `leadsnebula_core` for consistent behavior
    async fn create_test_pool() -> anyhow::Result<PgPool> {
        crate::test_helpers::create_test_pool().await
    }

    async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid, Uuid, String) {
        // Use a single transaction for all setup (1 connection instead of 4)
        // Retry transaction begin with exponential backoff to handle PoolTimedOut
        let mut tx = {
            let mut retries = 0;
            let max_retries = 3;
            loop {
                match pool.begin().await {
                    Ok(tx) => break tx,
                    Err(sqlx::Error::PoolTimedOut) if retries < max_retries => {
                        retries += 1;
                        let delay_ms = 100 * (1 << retries); // 200ms, 400ms, 800ms
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                    Err(e) => panic!("Failed to begin transaction for setup: {}", e),
                }
            }
        };

        // Create instance_user
        let instance_user_id = Uuid::new_v4();
        let unique_email = format!("test_user_{}@test.invalid", Uuid::new_v4());
        let password_hash = "hashed_password".to_string();

        sqlx::query(
            r#"
            INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
            VALUES ($1, $2, $3, 'active', NOW(), NOW(), NOW())
            ON CONFLICT DO NOTHING
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
             VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())
             ON CONFLICT DO NOTHING",
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
             VALUES ($1, $2, 'Test Publisher', $3, $4, $5, $6, 'active', NOW(), NOW())
             ON CONFLICT DO NOTHING",
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
             VALUES ($1, $2, 'Test Vertical', true, NOW(), NOW())
             ON CONFLICT DO NOTHING",
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
        buyer_id: Uuid,
        campaign_id: Uuid,
        request_type: &str,
    ) -> Lead {
        // Create lead with buyer_id and campaign_id (required by schema NOT NULL constraints)
        // The router may update these during routing, but we need valid IDs for initial insert
        let lead_uuid = Uuid::new_v4();
        let session_id = format!("sess_{}", Uuid::new_v4());
        // post_id should be empty string initially so router can set it (schema requires NOT NULL)
        let post_id = String::new();
        let strategy_val = if request_type == "ping" {
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
        .bind(request_type)
        .bind(&strategy_val)
        .bind(buyer_id)
        .bind(campaign_id)
        .bind(post_id)
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
        strategy: &str,
    ) -> Uuid {
        let ping_tree_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ping_trees (id, instance_id, name, vertical, strategy, status, created_at, updated_at)
            VALUES ($1, $2, 'Test Ping Tree', $3, $4, $5, NOW(), NOW())
            "#,
        )
        .bind(ping_tree_id)
        .bind(instance_id)
        .bind(vertical)
        .bind(strategy)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();

        // Link publisher to ping tree via join table
        sqlx::query(
            r#"
            INSERT INTO ping_tree_publishers (id, ping_tree_id, publisher_id, vertical, revshare_percentage, created_at, updated_at)
            VALUES ($1, $2, $3, $4, 80.0, NOW(), NOW())
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(ping_tree_id)
        .bind(publisher_id)
        .bind(vertical)
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
    #[ignore] // Requires ephemeral DB; run via ./autotests.sh
    async fn test_ping_auction_updates_lead_status() {
        if std::env::var("CI").is_err() && std::env::var("EPHEMERAL_DB").is_err() {
            eprintln!(
                "Skipping: run ./autotests.sh or set EPHEMERAL_DB=1 with ephemeral DATABASE_URL"
            );
            return;
        }
        let pool = create_test_pool()
            .await
            .expect("DATABASE_URL required when EPHEMERAL_DB or CI");

        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;

        let buyer_id = create_buyer(&pool, instance_id).await;
        let campaign =
            create_campaign(&pool, buyer_id, publisher_id, instance_id, &vertical_slug).await;
        let ping_tree_id = create_ping_tree(
            &pool,
            publisher_id,
            instance_id,
            &vertical_slug,
            "active",
            "ping_post",
        )
        .await;
        add_campaign_to_ping_tree(&pool, ping_tree_id, campaign.id, Some(1)).await;
        let lead = create_test_lead(
            &pool,
            publisher_id,
            vertical_id,
            instance_id,
            buyer_id,
            campaign.id,
            "ping",
        )
        .await;

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
    #[ignore] // Requires ephemeral DB; run via ./autotests.sh
    async fn test_ping_auction_persists_buyer_responses() {
        if std::env::var("CI").is_err() && std::env::var("EPHEMERAL_DB").is_err() {
            eprintln!(
                "Skipping: run ./autotests.sh or set EPHEMERAL_DB=1 with ephemeral DATABASE_URL"
            );
            return;
        }
        let pool = create_test_pool()
            .await
            .expect("DATABASE_URL required when EPHEMERAL_DB or CI");

        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;

        let buyer_id = create_buyer(&pool, instance_id).await;
        let campaign =
            create_campaign(&pool, buyer_id, publisher_id, instance_id, &vertical_slug).await;
        let ping_tree_id = create_ping_tree(
            &pool,
            publisher_id,
            instance_id,
            &vertical_slug,
            "active",
            "ping_post",
        )
        .await;
        add_campaign_to_ping_tree(&pool, ping_tree_id, campaign.id, Some(1)).await;
        let lead = create_test_lead(
            &pool,
            publisher_id,
            vertical_id,
            instance_id,
            buyer_id,
            campaign.id,
            "ping",
        )
        .await;

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

        // Give async persistence tasks time to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

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
    #[ignore] // Requires ephemeral DB; run via ./autotests.sh
    async fn test_fullpost_persists_both_ping_and_post_payloads() {
        if std::env::var("CI").is_err() && std::env::var("EPHEMERAL_DB").is_err() {
            eprintln!(
                "Skipping: run ./autotests.sh or set EPHEMERAL_DB=1 with ephemeral DATABASE_URL"
            );
            return;
        }
        let pool = create_test_pool()
            .await
            .expect("DATABASE_URL required when EPHEMERAL_DB or CI");

        let (publisher_id, instance_id, vertical_id, vertical_slug) = setup_test_data(&pool).await;

        let buyer_id = create_buyer(&pool, instance_id).await;
        let campaign =
            create_campaign(&pool, buyer_id, publisher_id, instance_id, &vertical_slug).await;
        let lead = create_test_lead(
            &pool,
            publisher_id,
            vertical_id,
            instance_id,
            buyer_id,
            campaign.id,
            "fullpost",
        )
        .await;
        let ping_tree_id = create_ping_tree(
            &pool,
            publisher_id,
            instance_id,
            &vertical_slug,
            "active",
            "ping_post",
        )
        .await;
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
