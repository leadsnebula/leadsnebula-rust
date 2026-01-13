// Edge case tests for BuyerRouter
// Tests error handling, timeout scenarios, and edge cases

#[cfg(test)]
mod buyer_router_edge_case_tests {
    use crate::models::campaign::Campaign;
    use crate::models::enums::CampaignStatus;
    use crate::services::buyer_router::BuyerRouter;
    use uuid::Uuid;

    fn sample_campaign() -> Campaign {
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

    fn sample_lead() -> crate::models::lead::Lead {
        crate::models::lead::Lead {
            uuid: Uuid::new_v4(),
            event_id: "evt_1".to_string(),
            lead_id: None,
            publisher_id: Some(Uuid::new_v4()),
            vertical_id: Uuid::new_v4(),
            campaign_id: None,
            buyer_id: None,
            request_type: "ping".to_string(),
            strategy: "default".to_string(),
            status: crate::models::enums::LeadStatus::Processing,
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

    #[tokio::test]
    async fn test_buyer_router_no_campaigns() {
        let lead = sample_lead();
        let router = BuyerRouter::new(lead, vec![], "ping".to_string());
        let result = router.route().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No campaign provided"));
    }

    #[tokio::test]
    async fn test_buyer_router_ping_returns_required_fields() {
        let lead = sample_lead();
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "ping".to_string());
        let resp = router.route().await.expect("route ping should succeed");

        assert!(resp.success);
        assert!(resp.ping_id.is_some());
        assert!(resp.promise_id.is_some());
        assert!(resp.price.is_some());
        assert_eq!(resp.status, "accepted");
    }

    #[tokio::test]
    async fn test_buyer_router_post_without_promise_id_fails() {
        let mut lead = sample_lead();
        lead.promise_id = None; // Explicitly no promise_id
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "post".to_string());
        let result = router.route().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing promise_id"));
    }

    #[tokio::test]
    async fn test_buyer_router_post_with_promise_id_succeeds() {
        let mut lead = sample_lead();
        lead.promise_id = Some("PROMISE_123".to_string());
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "post".to_string());
        let resp = router.route().await.expect("route post should succeed");

        assert!(resp.success);
        assert!(resp.post_id.is_some());
        assert_eq!(resp.promise_id, Some("PROMISE_123".to_string()));
    }

    #[tokio::test]
    async fn test_buyer_router_fullpost_ping_then_post() {
        let mut lead = sample_lead();
        lead.promise_id = None; // Start without promise_id
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "fullpost".to_string());

        // Fullpost should fail because ping doesn't update lead's promise_id
        // (BuyerRouter doesn't have access to database to update lead)
        let result = router.route().await;
        assert!(result.is_err()); // Should fail at post stage due to missing promise_id
    }

    #[tokio::test]
    async fn test_buyer_router_unknown_request_type() {
        let lead = sample_lead();
        let campaign = sample_campaign();
        let router = BuyerRouter::new(lead, vec![campaign], "unknown".to_string());
        let resp = router.route().await.expect("should return BuyerResponse");

        assert!(!resp.success);
        assert_eq!(resp.status, "error");
        assert!(resp.error.is_some());
        assert!(resp.error.unwrap().contains("Unknown request_type"));
    }
}
