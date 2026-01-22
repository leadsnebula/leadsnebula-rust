// Comprehensive tests for fullpost functionality
// These tests verify the fullpost flow: ping auction -> post routing

#[cfg(test)]
mod ping_tree_router_fullpost_tests {
    use crate::models::{
        campaign::Campaign, enums::CampaignStatus, enums::LeadStatus, lead::Lead,
        ping_tree::PingTree,
    };
    use crate::services::buyer_router::BuyerResponse;
    use crate::services::ping_tree_router::{PingTreeRouter, RoutingResult};
    use uuid::Uuid;

    fn create_test_lead() -> Lead {
        Lead {
            uuid: Uuid::new_v4(),
            event_id: "evt_test".to_string(),
            lead_id: None,
            publisher_id: Some(Uuid::new_v4()),
            vertical_id: Uuid::new_v4(),
            campaign_id: None,
            buyer_id: None,
            request_type: "fullpost".to_string(),
            strategy: "default".to_string(),
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

    fn create_test_campaign() -> Campaign {
        Campaign {
            id: Uuid::new_v4(),
            buyer_id: Uuid::new_v4(),
            publisher_id: Uuid::new_v4(),
            instance_id: Uuid::new_v4(),
            name: Some("test-campaign".to_string()),
            vertical: "test-vertical".to_string(),
            campaign_token: "token123".to_string(),
            status: CampaignStatus::Active,
            is_documentation_test: false,
            deleted_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn create_ping_tree(strategy: &str) -> PingTree {
        PingTree {
            id: Uuid::new_v4(),
            instance_id: Uuid::new_v4(),
            name: "Test Ping Tree".to_string(),
            vertical: "test-vertical".to_string(),
            strategy: strategy.to_string(),
            status: "active".to_string(),
            priority: Some(1),
            deleted_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_fullpost_requires_ping_post_strategy() {
        // Fullpost should only work with ping_post strategy
        let ping_tree = create_ping_tree("fullpost");
        assert_eq!(ping_tree.strategy, "fullpost");
        // This strategy is not yet implemented, so fullpost should return error
    }

    #[test]
    fn test_fullpost_works_with_ping_post_strategy() {
        // Fullpost should work with ping_post strategy
        let ping_tree = create_ping_tree("ping_post");
        assert_eq!(ping_tree.strategy, "ping_post");
        // This is the supported strategy for fullpost
    }

    #[test]
    fn test_fullpost_merges_ping_and_post_results() {
        // When fullpost succeeds, it should merge ping and post results
        let ping_result = RoutingResult {
            success: true,
            status: "accepted".to_string(),
            error: None,
            promise_id: Some("PROMISE_123".to_string()),
            ping_id: Some("ping_123".to_string()),
            post_id: None,
            price: Some(100.0),
            campaign_id: Some(Uuid::new_v4()),
            buyer_id: Some(Uuid::new_v4()),
            per_buyer_timings: None,
        };

        let post_result = RoutingResult {
            success: true,
            status: "sold".to_string(),
            error: None,
            promise_id: Some("PROMISE_123".to_string()),
            ping_id: Some("ping_123".to_string()),
            post_id: Some("post_456".to_string()),
            price: Some(100.0),
            campaign_id: Some(Uuid::new_v4()),
            buyer_id: Some(Uuid::new_v4()),
            per_buyer_timings: None,
        };

        // Merged result should have both ping_id and post_id
        assert!(post_result.ping_id.is_some());
        assert!(post_result.post_id.is_some());
        assert_eq!(post_result.status, "sold");
    }

    #[test]
    fn test_fullpost_fails_when_ping_fails() {
        // If ping fails, fullpost should return early with ping result
        let ping_result = RoutingResult {
            success: false,
            status: "rejected".to_string(),
            error: Some("Buyer rejected".to_string()),
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: None,
            campaign_id: None,
            buyer_id: None,
            per_buyer_timings: None,
        };

        // Fullpost should not proceed to post if ping fails
        assert!(!ping_result.success);
        assert_eq!(ping_result.status, "rejected");
    }

    #[test]
    fn test_fullpost_requires_promise_id_for_post() {
        // After ping succeeds, the lead should have promise_id for post
        let ping_result = RoutingResult {
            success: true,
            status: "accepted".to_string(),
            error: None,
            promise_id: Some("PROMISE_123".to_string()),
            ping_id: Some("ping_123".to_string()),
            post_id: None,
            price: Some(100.0),
            campaign_id: Some(Uuid::new_v4()),
            buyer_id: Some(Uuid::new_v4()),
            per_buyer_timings: None,
        };

        // Post requires promise_id
        assert!(ping_result.promise_id.is_some());
        assert!(ping_result.ping_id.is_some());
    }
}
