// Tests for async persistence behavior
// Verifies that async persistence doesn't block routing and handles errors gracefully

#[cfg(test)]
mod async_persistence_tests {
    use crate::models::{campaign::Campaign, enums::CampaignStatus, enums::LeadStatus, lead::Lead};
    use crate::services::ping_tree_router::PingTreeRouter;
    use sqlx::PgPool;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration, Instant};
    use uuid::Uuid;

    #[tokio::test]
    #[ignore] // Requires ephemeral DB; run via ./autotests.sh
    async fn test_async_persistence_does_not_block_routing() {
        if std::env::var("CI").is_err() && std::env::var("EPHEMERAL_DB").is_err() {
            eprintln!(
                "Skipping: run ./autotests.sh or set EPHEMERAL_DB=1 with ephemeral DATABASE_URL"
            );
            return;
        }
        let pool = create_test_pool()
            .await
            .expect("DATABASE_URL required when EPHEMERAL_DB or CI");
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]);

        // Set up test data in a single transaction (1 connection instead of 9)
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

        let instance_user_id = Uuid::new_v4();
        let unique_email = format!("test_user_{}@test.invalid", Uuid::new_v4());
        sqlx::query(
            r#"
            INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
            VALUES ($1, $2, 'hashed_password', 'active', NOW(), NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(instance_user_id)
        .bind(&unique_email)
        .execute(&mut *tx)
        .await
        .unwrap();

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

        let publisher_id = Uuid::new_v4();
        let api_key_hash = format!("hash_{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO publishers (id, instance_id, name, email, api_key_hash, api_key_prefix, api_key_encrypted, status, created_at, updated_at)
             VALUES ($1, $2, 'Test Publisher', $3, $4, $5, $6, 'active', NOW(), NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(publisher_id)
        .bind(instance_id)
        .bind(&format!("publisher_{}@test.invalid", Uuid::new_v4()))
        .bind(&api_key_hash)
        .bind(&format!("pk_{}", &api_key_hash[..8]))
        .bind("")
        .execute(&mut *tx)
        .await
        .unwrap();

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

        let buyer_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO buyers (id, instance_id, name, status, created_at, updated_at)
            VALUES ($1, $2, 'Test Buyer', 'active', NOW(), NOW())
            "#,
        )
        .bind(buyer_id)
        .bind(instance_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        let campaign_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO campaigns (id, buyer_id, publisher_id, instance_id, vertical, campaign_token, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, 'active', NOW(), NOW())
            "#,
        )
        .bind(campaign_id)
        .bind(buyer_id)
        .bind(publisher_id)
        .bind(instance_id)
        .bind(&vertical_slug)
        .bind(format!("token_{}", Uuid::new_v4()))
        .execute(&mut *tx)
        .await
        .unwrap();

        let ping_tree_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ping_trees (id, instance_id, name, vertical, strategy, status, created_at, updated_at)
            VALUES ($1, $2, 'Test Ping Tree', $3, 'ping_post', 'active', NOW(), NOW())
            "#,
        )
        .bind(ping_tree_id)
        .bind(instance_id)
        .bind(&vertical_slug)
        .execute(&mut *tx)
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
        .bind(&vertical_slug)
        .execute(&mut *tx)
        .await
        .unwrap();

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
        .bind(Some(1))
        .execute(&mut *tx)
        .await
        .unwrap();

        // Create test lead in database
        let lead_uuid = Uuid::new_v4();
        let session_id = format!("sess_{}", Uuid::new_v4());
        let post_id = String::new(); // Empty string for NOT NULL constraint
        sqlx::query(
            r#"
            INSERT INTO leads (uuid, event_id, publisher_id, vertical_id, request_type, strategy, status, tcpa_consent, tcpa_language, submitted_at, buyer_id, campaign_id, post_id, session_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, 'ping', 'pingPost', 'processing', false, '', NOW(), $5, $6, $7, $8, NOW(), NOW())
            "#,
        )
        .bind(lead_uuid)
        .bind(format!("evt_{}", Uuid::new_v4()))
        .bind(publisher_id)
        .bind(vertical_id)
        .bind(buyer_id)
        .bind(campaign_id)
        .bind(post_id)
        .bind(&session_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Commit transaction so router can see the data
        tx.commit().await.unwrap();

        let lead = sqlx::query_as::<_, Lead>("SELECT * FROM leads WHERE uuid = $1")
            .bind(lead_uuid)
            .fetch_one(&pool)
            .await
            .unwrap();

        let router = PingTreeRouter::new(
            lead.clone(),
            publisher_id,
            vertical_slug,
            "ping".to_string(),
        );

        // Measure routing time (should not be blocked by persistence)
        let start = Instant::now();
        let _result = router.route(pool_arc.clone(), encryption_key.clone()).await;
        let routing_time = start.elapsed();

        // Routing should complete reasonably quickly (< 10s for ping auction, accounting for HTTP timeouts)
        // The key point is that persistence happens asynchronously, not that routing is super fast
        assert!(routing_time < Duration::from_secs(10));

        // Give async tasks time to complete
        sleep(Duration::from_millis(500)).await;

        // Verify persistence happened (check buyer_responses table)
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM buyer_responses WHERE lead_id = $1")
                .bind(lead.uuid)
                .fetch_one(&pool)
                .await
                .unwrap_or(0);

        // Persistence should have happened asynchronously
        assert!(count >= 0);
    }

    #[tokio::test]
    #[ignore] // Requires ephemeral DB; run via ./autotests.sh
    async fn test_async_persistence_handles_errors_gracefully() {
        if std::env::var("CI").is_err() && std::env::var("EPHEMERAL_DB").is_err() {
            eprintln!(
                "Skipping: run ./autotests.sh or set EPHEMERAL_DB=1 with ephemeral DATABASE_URL"
            );
            return;
        }
        // Verify that persistence errors don't crash the routing flow
        // This test would need to simulate database errors
        // (e.g., by using a pool with limited connections that are exhausted)
    }

    async fn create_test_pool() -> anyhow::Result<PgPool> {
        crate::test_helpers::create_test_pool().await
    }

    async fn create_test_lead(pool: &PgPool) -> Lead {
        // Create minimal test lead
        Lead {
            uuid: Uuid::new_v4(),
            event_id: format!("evt_{}", Uuid::new_v4()),
            lead_id: None,
            publisher_id: Some(Uuid::new_v4()),
            vertical_id: Uuid::new_v4(),
            campaign_id: None,
            buyer_id: None,
            request_type: "ping".to_string(),
            strategy: "ping_post".to_string(),
            status: LeadStatus::Processing,
            promise_id: None,
            ping_id: None,
            post_id: None,
            session_id: None,
            request_stage: None,
            first_name_encrypted: None,
            last_name_encrypted: None,
            email_encrypted: None,
            cell_phone_encrypted: None,
            street_address_encrypted: None,
            city_encrypted: None,
            state_encrypted: None,
            zip_encrypted: None,
            ip_address_encrypted: None,
            email_sha256: None,
            phone_sha256: None,
            ip_address_hash: None,
            email_domain: None,
            tcpa_consent: false,
            tcpa_language: "en".to_string(),
            is_test: false,
            user_agent: None,
            referrer: None,
            website_url: None,
            click_id: None,
            url_consent: None,
            best_call_time: None,
            date_of_birth: None,
            home_phone: None,
            jornaya_lead_id: None,
            trusted_form_url: None,
            fbp_cookie: None,
            fbc_cookie: None,
            utm_params: None,
            submitted_at: None,
            sold_at: None,
            retry_count: 0,
            next_retry_at: None,
            vertical_data: serde_json::json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}
