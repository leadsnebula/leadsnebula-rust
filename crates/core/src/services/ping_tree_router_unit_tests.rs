// Comprehensive unit tests for PingTreeRouter
// These tests use mocks and don't require a database

#[cfg(test)]
mod ping_tree_router_unit_tests {
    use crate::models::{campaign::Campaign, enums::CampaignStatus, enums::LeadStatus, lead::Lead};
    use crate::services::buyer_router::BuyerResponse;
    use crate::services::ping_tree_router::PingTreeRouter;
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
            request_type: "ping".to_string(),
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

    #[test]
    fn test_map_ping_status_to_lead_status_comprehensive() {
        // Test all status mappings
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("rejected", false),
            "rejected"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("declined", false),
            "rejected"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("denied", false),
            "rejected"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("accepted", true),
            "accepted"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("accepted", false),
            "error"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("timeout", false),
            "timeout"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("invalid", false),
            "invalid"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("invalid_lead", false),
            "invalid"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("validation_error", false),
            "invalid"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("error", false),
            "error"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("server_error", false),
            "error"
        );
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("internal_error", false),
            "error"
        );
        // Unknown status with success
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("weirdstatus", true),
            "ping_accepted"
        );
        // Unknown status without success
        assert_eq!(
            PingTreeRouter::map_ping_status_to_lead_status("weirdstatus", false),
            "error"
        );
    }

    #[test]
    fn test_select_winner_edge_cases() {
        use crate::services::ping_tree_router::select_winner_for_test;

        // Test with single response
        let campaign1 = Uuid::new_v4();
        let resp1 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: Some(100.0), // Set bid for ping auction
        };
        let responses = vec![(&resp1, campaign1, Some(1))];
        let (_winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign1);
        assert_eq!(_winner.price, Some(100.0));

        // Test with zero prices (should filter out)
        let campaign2 = Uuid::new_v4();
        let resp2 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(0.0),
            bid: Some(0.0), // Set bid for ping auction
        };
        let responses = vec![(&resp1, campaign1, None), (&resp2, campaign2, None)];
        let (_winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign1); // Only resp1 is valid

        // Test with negative prices (should filter out)
        let resp3 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(-10.0),
            bid: Some(-10.0), // Set bid for ping auction
        };
        let responses = vec![(&resp1, campaign1, None), (&resp3, campaign2, None)];
        let (_winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign1); // Only resp1 is valid

        // Test with very close prices (epsilon tie)
        let resp4 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.005),
            bid: Some(100.005), // Set bid for ping auction
        };
        let responses = vec![
            (&resp1, campaign1, Some(2)),
            (&resp4, campaign2, Some(1)), // Better priority
        ];
        let (_winner, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign2); // Should win due to priority
    }

    #[test]
    fn test_select_winner_random_tie_breaker() {
        use crate::services::ping_tree_router::select_winner_for_test;

        // Test random selection when prices and priorities are equal
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        let resp1 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: None,
        };
        let resp2 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: None,
        };
        let resp3 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: None,
        };

        let responses = vec![
            (&resp1, campaign1, None),
            (&resp2, campaign2, None),
            (&resp3, campaign3, None),
        ];

        // Run multiple times - should select one of the three (random)
        let mut selected_ids = std::collections::HashSet::new();
        for _ in 0..10 {
            let (_, winner_id, _) = select_winner_for_test(responses.clone());
            selected_ids.insert(winner_id);
        }

        // Should have selected at least one (ideally all three over many runs)
        assert!(!selected_ids.is_empty());
        assert!(selected_ids.len() <= 3);
    }

    #[test]
    fn test_select_winner_priority_ordering() {
        use crate::services::ping_tree_router::select_winner_for_test;

        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        // All same price, different priorities
        let resp1 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: Some(100.0), // Set bid for ping auction
        };
        let resp2 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: Some(100.0), // Set bid for ping auction
        };
        let resp3 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: Some(100.0), // Set bid for ping auction
        };

        let responses = vec![
            (&resp1, campaign1, Some(3)), // Lowest priority
            (&resp2, campaign2, Some(1)), // Highest priority (lowest number)
            (&resp3, campaign3, Some(2)),
        ];

        let (_, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign2); // Should win due to priority 1
    }

    #[test]
    fn test_select_winner_price_overrides_priority() {
        use crate::services::ping_tree_router::select_winner_for_test;

        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        // Higher price should win even with worse priority
        let resp1 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(150.0),
            bid: None,
        };
        let resp2 = BuyerResponse {
            success: true,
            status: "accepted".to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price: Some(100.0),
            bid: None,
        };

        let responses = vec![
            (&resp1, campaign1, Some(5)), // Higher price, worse priority
            (&resp2, campaign2, Some(1)), // Lower price, better priority
        ];

        let (_, winner_id, _) = select_winner_for_test(responses);
        assert_eq!(winner_id, campaign1); // Should win due to higher price
    }
}
