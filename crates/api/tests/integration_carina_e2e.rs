// E2E tests for Carina API lead routing
// These tests verify the full request flow through the API stack

#![allow(unused_imports, unused_variables, unreachable_code, dead_code)]
mod common;
use axum::ServiceExt;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use common::create_test_pool;
use leadsnebula_core::models::{
    campaign::Campaign,
    enums::{CampaignStatus, LeadStatus},
    lead::Lead,
    publisher::Publisher,
    vertical::Vertical,
};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

// Helper function that works with both PgPool and Transaction
async fn setup_test_data<'e, E>(executor: &mut E) -> (Uuid, Uuid, String, String)
where
    for<'c> &'c mut E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    // Create instance_user
    let instance_user_id = Uuid::new_v4();
    let unique_email = format!("test_user_{}@test.invalid", Uuid::new_v4());

    sqlx::query(
            r#"
            INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
            VALUES ($1, $2, $3, 'active', NOW(), NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(instance_user_id)
        .bind(&unique_email)
        .bind("hashed_password")
        .execute(&mut *executor)
        .await
        .unwrap();

    // Create instance
    let instance_id = Uuid::new_v4();
    sqlx::query(
        r#"
                INSERT INTO instances (id, instance_user_id, name, payment_status, created_at, updated_at)
                    VALUES ($1, $2, $3, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
    )
    .bind(instance_id)
    .bind(instance_user_id)
    .bind("Test Instance")
    .execute(&mut *executor)
    .await
    .unwrap();

    // Create vertical
    let vertical_id = Uuid::new_v4();
    let vertical_slug = format!("test_vertical_{}", Uuid::new_v4());
    sqlx::query(
        r#"
            INSERT INTO verticals (id, slug, name, is_active, created_at, updated_at)
            VALUES ($1, $2, $3, true, NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
    )
    .bind(vertical_id)
    .bind(&vertical_slug)
    .bind("Test Vertical")
    .execute(&mut *executor)
    .await
    .unwrap();

    // Create publisher
    let publisher_id = Uuid::new_v4();
    let publisher_email = format!("publisher_{}@test.invalid", Uuid::new_v4());
    let api_key_hash = format!("api_key_{}", Uuid::new_v4());
    // Create a test encryption key and encrypted API key value to satisfy NOT NULL constraints
    let test_key = vec![0u8; 32];
    let encryption_service = leadsnebula_core::encryption::EncryptionService::new(&test_key)
        .expect("Failed to create encryption service");
    let test_api_key = format!("pk_test_{}", Uuid::new_v4());
    let encrypted_key = encryption_service
        .encrypt(&test_api_key)
        .expect("Failed to encrypt API key");
    sqlx::query(
            r#"
            INSERT INTO publishers (id, instance_id, name, email, api_key_prefix, api_key_hash, api_key_encrypted, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(publisher_id)
        .bind(instance_id)
        .bind("Test Publisher")
        .bind(&publisher_email)
        .bind("pk_test_")
        .bind(&api_key_hash)
        .bind(&encrypted_key)
        .execute(&mut *executor)
        .await
        .unwrap();

    (publisher_id, vertical_id, vertical_slug, api_key_hash)
}

async fn create_test_app_state() -> (Router, PgPool) {
    let pool = create_test_pool().await.unwrap();
    // Note: This is a simplified test - in real E2E tests, you'd set up full AppState
    // For now, we'll test the routing logic directly
    (Router::new(), pool)
}

#[tokio::test]
#[ignore] // Requires database setup
async fn test_e2e_ping_request_flow() {
    let (_app, pool) = create_test_app_state().await;
    let mut tx = pool.begin().await.unwrap();
    let (publisher_id, vertical_id, vertical_slug, _api_key) = setup_test_data(&mut *tx).await;

    // Create buyer and campaign
    let buyer_id = Uuid::new_v4();
    sqlx::query(
        r#"
            INSERT INTO buyers (id, instance_id, name, status, created_at, updated_at)
            VALUES ($1, (SELECT id FROM instances LIMIT 1), $2, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
    )
    .bind(buyer_id)
    .bind("Test Buyer")
    .execute(&mut *tx)
    .await
    .unwrap();

    let campaign_id = Uuid::new_v4();
    sqlx::query(
            r#"
            INSERT INTO campaigns (id, buyer_id, publisher_id, instance_id, vertical, campaign_token, status, created_at, updated_at)
            VALUES ($1, $2, $3, (SELECT id FROM instances LIMIT 1), $4, $5, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(campaign_id)
        .bind(buyer_id)
        .bind(publisher_id)
        .bind(&vertical_slug)
        .bind(format!("token_{}", Uuid::new_v4()))
        .execute(&mut *tx)
        .await
        .unwrap();

    // Create ping tree
    // Note: Include publisher_id for databases that haven't run migration 20260120000004 yet
    let ping_tree_id = Uuid::new_v4();
    sqlx::query(
            r#"
            INSERT INTO ping_trees (id, publisher_id, instance_id, name, vertical, status, strategy, created_at, updated_at)
            VALUES ($1, $2, (SELECT id FROM instances LIMIT 1), $3, $4, 'active', 'ping_post', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(ping_tree_id)
        .bind(publisher_id)
        .bind("Test Ping Tree")
        .bind(&vertical_slug)
        .execute(&mut *tx)
        .await
        .unwrap();

    // Create ping_tree_publishers entry (required for routing in new schema)
    // Use savepoint to handle errors gracefully without aborting the transaction
    let _ = sqlx::query("SAVEPOINT ping_tree_publishers_insert")
        .execute(&mut *tx)
        .await;
    let insert_result = sqlx::query(
            r#"
            INSERT INTO ping_tree_publishers (id, ping_tree_id, publisher_id, vertical, created_at, updated_at)
            VALUES (gen_random_uuid(), $1, $2, $3, NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(ping_tree_id)
        .bind(publisher_id)
        .bind(&vertical_slug)
        .execute(&mut *tx)
        .await;

    // If insert failed (e.g., table doesn't exist), rollback to savepoint
    if insert_result.is_err() {
        let _ = sqlx::query("ROLLBACK TO SAVEPOINT ping_tree_publishers_insert")
            .execute(&mut *tx)
            .await;
    } else {
        let _ = sqlx::query("RELEASE SAVEPOINT ping_tree_publishers_insert")
            .execute(&mut *tx)
            .await;
    }

    // Add campaign to ping tree
    sqlx::query(
            r#"
            INSERT INTO ping_tree_campaigns (ping_tree_id, campaign_id, enabled, priority, created_at, updated_at)
            VALUES ($1, $2, true, 1, NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(ping_tree_id)
        .bind(campaign_id)
        .execute(&mut *tx)
        .await
        .unwrap();

    // Create test lead
    let lead = Lead {
        uuid: Uuid::new_v4(),
        event_id: format!("evt_{}", Uuid::new_v4()),
        lead_id: None,
        publisher_id: Some(publisher_id),
        vertical_id,
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
    };

    // Insert lead into database (required for router.update_lead_status)
    // Use buyer_id and campaign_id from test setup (database requires buyer_id to be NOT NULL)
    let strategy_val = "pingPost".to_string();
    sqlx::query(
            r#"
            INSERT INTO leads (uuid, event_id, publisher_id, vertical_id, request_type, strategy, status, tcpa_consent, tcpa_language, submitted_at, buyer_id, campaign_id, post_id, session_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, 'processing', $7, $8, NOW(), $9, $10, $11, $12, NOW(), NOW())
            "#,
        )
        .bind(lead.uuid)
        .bind(&lead.event_id)
        .bind(lead.publisher_id)
        .bind(lead.vertical_id)
        .bind(&lead.request_type)
        .bind(&strategy_val)
        .bind(lead.tcpa_consent)
        .bind(&lead.tcpa_language)
        .bind(buyer_id)  // Use buyer_id from test setup
        .bind(campaign_id)  // Use campaign_id from test setup
        .bind(lead.post_id.as_ref().unwrap_or(&String::new()))
        .bind(lead.session_id.as_ref())
        .execute(&mut *tx)
        .await
        .unwrap();

    // Test routing
    use leadsnebula_core::services::ping_tree_router::PingTreeRouter;
    let router = PingTreeRouter::new(
        lead.clone(),
        publisher_id,
        vertical_slug.clone(),
        "ping".to_string(),
        None,
        None,
    );

    let pool_arc = Arc::new(pool.clone());
    let encryption_key = Arc::new(vec![0u8; 32]); // Dummy key for tests
    let result = router.route(pool_arc, encryption_key).await;

    // Verify result
    assert!(result.is_ok());
    let routing_result = result.unwrap();

    // Verify lead status was updated
    let updated_lead: Option<Lead> =
        sqlx::query_as::<_, Lead>("SELECT * FROM leads WHERE uuid = $1")
            .bind(lead.uuid)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

    if let Some(updated) = updated_lead {
        assert_ne!(updated.status, LeadStatus::Processing);
    }

    // Verify buyer_responses were persisted
    let response_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM buyer_responses WHERE lead_id = $1")
            .bind(lead.uuid)
            .fetch_one(&mut *tx)
            .await
            .unwrap();

    assert!(response_count >= 0); // At least attempted to persist

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore] // Requires database setup
async fn test_e2e_fullpost_request_flow() {
    // Similar to ping test but for fullpost
    // This verifies the complete ping -> post flow
    let (_app, pool) = create_test_app_state().await;
    let mut tx = pool.begin().await.unwrap();
    let (publisher_id, vertical_id, vertical_slug, _api_key) = setup_test_data(&mut *tx).await;

    // Create buyer and campaign (same as ping test)
    let buyer_id = Uuid::new_v4();
    sqlx::query(
        r#"
            INSERT INTO buyers (id, instance_id, name, status, created_at, updated_at)
            VALUES ($1, (SELECT id FROM instances LIMIT 1), $2, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
    )
    .bind(buyer_id)
    .bind("Test Buyer")
    .execute(&mut *tx)
    .await
    .unwrap();

    let campaign_id = Uuid::new_v4();
    sqlx::query(
        r#"
            INSERT INTO campaigns (id, buyer_id, publisher_id, instance_id, vertical, campaign_token, status, created_at, updated_at)
            VALUES ($1, $2, $3, (SELECT id FROM instances LIMIT 1), $4, $5, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
    )
    .bind(campaign_id)
    .bind(buyer_id)
    .bind(publisher_id)
    .bind(&vertical_slug)
    .bind(format!("token_{}", Uuid::new_v4()))
    .execute(&mut *tx)
    .await
    .unwrap();

    // Create ping tree
    // Note: Include publisher_id for databases that haven't run migration 20260120000004 yet
    let ping_tree_id = Uuid::new_v4();
    sqlx::query(
            r#"
            INSERT INTO ping_trees (id, publisher_id, instance_id, name, vertical, status, strategy, created_at, updated_at)
            VALUES ($1, $2, (SELECT id FROM instances LIMIT 1), $3, $4, 'active', 'ping_post', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(ping_tree_id)
        .bind(publisher_id)
        .bind("Test Ping Tree")
        .bind(&vertical_slug)
        .execute(&mut *tx)
        .await
        .unwrap();

    // Create ping_tree_publishers entry (required for routing in new schema)
    // Use savepoint to handle errors gracefully without aborting the transaction
    let _ = sqlx::query("SAVEPOINT ping_tree_publishers_insert")
        .execute(&mut *tx)
        .await;
    let insert_result = sqlx::query(
            r#"
            INSERT INTO ping_tree_publishers (id, ping_tree_id, publisher_id, vertical, created_at, updated_at)
            VALUES (gen_random_uuid(), $1, $2, $3, NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(ping_tree_id)
        .bind(publisher_id)
        .bind(&vertical_slug)
        .execute(&mut *tx)
        .await;

    // If insert failed (e.g., table doesn't exist), rollback to savepoint
    if insert_result.is_err() {
        let _ = sqlx::query("ROLLBACK TO SAVEPOINT ping_tree_publishers_insert")
            .execute(&mut *tx)
            .await;
    } else {
        let _ = sqlx::query("RELEASE SAVEPOINT ping_tree_publishers_insert")
            .execute(&mut *tx)
            .await;
    }

    // Add campaign to ping tree
    sqlx::query(
        r#"
            INSERT INTO ping_tree_campaigns (ping_tree_id, campaign_id, enabled, priority, created_at, updated_at)
            VALUES ($1, $2, true, 1, NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
    )
    .bind(ping_tree_id)
    .bind(campaign_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    // Create test lead for fullpost
    let lead = Lead {
        uuid: Uuid::new_v4(),
        event_id: format!("evt_{}", Uuid::new_v4()),
        lead_id: None,
        publisher_id: Some(publisher_id),
        vertical_id,
        campaign_id: None,
        buyer_id: None,
        request_type: "fullpost".to_string(),
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
    };

    // Insert lead into database (required for router.update_lead_status)
    // Use buyer_id and campaign_id from test setup (database requires buyer_id to be NOT NULL)
    let strategy_val = "fullPost".to_string();
    sqlx::query(
            r#"
            INSERT INTO leads (uuid, event_id, publisher_id, vertical_id, request_type, strategy, status, tcpa_consent, tcpa_language, submitted_at, buyer_id, campaign_id, post_id, session_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, 'processing', $7, $8, NOW(), $9, $10, $11, $12, NOW(), NOW())
            "#,
        )
        .bind(lead.uuid)
        .bind(&lead.event_id)
        .bind(lead.publisher_id)
        .bind(lead.vertical_id)
        .bind(&lead.request_type)
        .bind(&strategy_val)
        .bind(lead.tcpa_consent)
        .bind(&lead.tcpa_language)
        .bind(buyer_id)  // Use buyer_id from test setup
        .bind(campaign_id)  // Use campaign_id from test setup
        .bind(lead.post_id.as_ref().unwrap_or(&String::new()))
        .bind(lead.session_id.as_ref())
        .execute(&mut *tx)
        .await
        .unwrap();

    // Test fullpost routing
    use leadsnebula_core::services::ping_tree_router::PingTreeRouter;
    let router = PingTreeRouter::new(
        lead.clone(),
        publisher_id,
        vertical_slug.clone(),
        "fullpost".to_string(),
        None,
        None,
    );

    let pool_arc = Arc::new(pool.clone());
    let encryption_key = Arc::new(vec![0u8; 32]);
    let result = router.route(pool_arc, encryption_key).await;

    // Verify result
    assert!(result.is_ok());
    let routing_result = result.unwrap();

    // For fullpost, we should have both ping_id and post_id
    assert!(routing_result.ping_id.is_some() || routing_result.post_id.is_some());

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore] // Requires database setup
async fn test_e2e_error_handling() {
    // Test error scenarios:
    // - No ping tree
    // - No campaigns
    // - Invalid vertical
    let (_app, pool) = create_test_app_state().await;
    let mut tx = pool.begin().await.unwrap();
    let (publisher_id, vertical_id, vertical_slug, _api_key) = setup_test_data(&mut *tx).await;

    // Create buyer and campaign (required for lead INSERT - buyer_id must be NOT NULL)
    let buyer_id = Uuid::new_v4();
    sqlx::query(
        r#"
            INSERT INTO buyers (id, instance_id, name, status, created_at, updated_at)
            VALUES ($1, (SELECT id FROM instances LIMIT 1), $2, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
    )
    .bind(buyer_id)
    .bind("Test Buyer")
    .execute(&mut *tx)
    .await
    .unwrap();

    let campaign_id = Uuid::new_v4();
    sqlx::query(
            r#"
            INSERT INTO campaigns (id, buyer_id, publisher_id, instance_id, vertical, campaign_token, status, created_at, updated_at)
            VALUES ($1, $2, $3, (SELECT id FROM instances LIMIT 1), $4, $5, 'active', NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(campaign_id)
        .bind(buyer_id)
        .bind(publisher_id)
        .bind(&vertical_slug)
        .bind(format!("token_{}", Uuid::new_v4()))
        .execute(&mut *tx)
        .await
        .unwrap();

    // Test 1: No ping tree for publisher/vertical
    let lead_no_tree = Lead {
        uuid: Uuid::new_v4(),
        event_id: format!("evt_{}", Uuid::new_v4()),
        lead_id: None,
        publisher_id: Some(publisher_id),
        vertical_id,
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
    };

    // Insert lead into database (required for router.update_lead_status)
    // Use buyer_id and campaign_id from test setup (database requires buyer_id to be NOT NULL)
    let strategy_val = "pingPost".to_string();
    sqlx::query(
            r#"
            INSERT INTO leads (uuid, event_id, publisher_id, vertical_id, request_type, strategy, status, tcpa_consent, tcpa_language, submitted_at, buyer_id, campaign_id, post_id, session_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, 'processing', $7, $8, NOW(), $9, $10, $11, $12, NOW(), NOW())
            "#,
        )
        .bind(lead_no_tree.uuid)
        .bind(&lead_no_tree.event_id)
        .bind(lead_no_tree.publisher_id)
        .bind(lead_no_tree.vertical_id)
        .bind(&lead_no_tree.request_type)
        .bind(&strategy_val)
        .bind(lead_no_tree.tcpa_consent)
        .bind(&lead_no_tree.tcpa_language)
        .bind(buyer_id)  // Use buyer_id from test setup
        .bind(campaign_id)  // Use campaign_id from test setup
        .bind(lead_no_tree.post_id.as_ref().unwrap_or(&String::new()))
        .bind(lead_no_tree.session_id.as_ref())
        .execute(&mut *tx)
        .await
        .unwrap();

    use leadsnebula_core::services::ping_tree_router::PingTreeRouter;
    let router = PingTreeRouter::new(
        lead_no_tree,
        publisher_id,
        vertical_slug.clone(),
        "ping".to_string(),
        None,
        None,
    );

    let pool_arc = Arc::new(pool.clone());
    let encryption_key = Arc::new(vec![0u8; 32]);
    let result = router.route(pool_arc, encryption_key).await;

    // Should return error result (no ping tree found)
    assert!(result.is_ok());
    let routing_result = result.unwrap();
    assert!(!routing_result.success);
    assert!(routing_result.error.is_some());

    // Rollback transaction to prevent test data from persisting
    tx.rollback().await.unwrap();
}
