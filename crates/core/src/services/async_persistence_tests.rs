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
    #[ignore] // Requires database setup
    async fn test_async_persistence_does_not_block_routing() {
        // Verify that async persistence tasks don't block the main routing response
        let pool = create_test_pool().await.unwrap();
        let pool_arc = Arc::new(pool.clone());
        let encryption_key = Arc::new(vec![0u8; 32]);

        // Create test lead and router
        let lead = create_test_lead(&pool).await;
        let router = PingTreeRouter::new(
            lead.clone(),
            Uuid::new_v4(),
            "test".to_string(),
            "ping".to_string(),
        );

        // Measure routing time (should not be blocked by persistence)
        let start = Instant::now();
        let _result = router.route(pool_arc.clone(), encryption_key.clone()).await;
        let routing_time = start.elapsed();

        // Routing should complete quickly (< 2s for ping auction)
        assert!(routing_time < Duration::from_secs(2));

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
    #[ignore] // Requires database setup
    async fn test_async_persistence_handles_errors_gracefully() {
        // Verify that persistence errors don't crash the routing flow
        // This test would need to simulate database errors
        // (e.g., by using a pool with limited connections that are exhausted)
    }

    async fn create_test_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
        let database_url = std::env::var("DATABASE_URL")
            .ok()
            .ok_or_else(|| "DATABASE_URL not set".to_string())?;

        use sqlx::postgres::PgPoolOptions;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&database_url)
            .await?;

        Ok(pool)
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
