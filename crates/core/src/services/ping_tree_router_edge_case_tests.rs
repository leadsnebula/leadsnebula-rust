// Edge case tests for ping auction, post routing, and error handling
// These tests cover timeout scenarios, all rejections, mixed responses, etc.

#[cfg(test)]
mod ping_tree_router_edge_case_tests {
    use crate::services::buyer_router::BuyerResponse;
    use crate::services::ping_tree_router::select_winner_for_test;
    use uuid::Uuid;

    fn create_buyer_response(success: bool, price: Option<f64>, status: &str) -> BuyerResponse {
        // For ping auctions, use bid; for post auctions, use price
        // Since select_winner filters by bid, we need to set bid for ping auction tests
        BuyerResponse {
            success,
            status: status.to_string(),
            error: None,
            message: None,
            promise_id: None,
            ping_id: None,
            post_id: None,
            price,
            bid: price, // Use price as bid for ping auction tests
        }
    }

    #[test]
    fn test_select_winner_all_timeouts() {
        // When all responses timeout, should return first error response
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        let resp1 = create_buyer_response(false, None, "timeout");
        let resp2 = create_buyer_response(false, None, "timeout");

        let responses = vec![(&resp1, campaign1, None), (&resp2, campaign2, None)];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should return first error response
        assert_eq!(winner_id, campaign1);
    }

    #[test]
    fn test_select_winner_all_rejections() {
        // When all responses are rejected, should return first error response
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        let resp1 = create_buyer_response(false, Some(0.0), "rejected");
        let resp2 = create_buyer_response(false, Some(0.0), "rejected");

        let responses = vec![(&resp1, campaign1, None), (&resp2, campaign2, None)];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should return first error response
        assert_eq!(winner_id, campaign1);
    }

    #[test]
    fn test_select_winner_mixed_responses() {
        // Mix of success, timeout, and rejection
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(150.0), "accepted");
        let resp2 = create_buyer_response(false, None, "timeout");
        let resp3 = create_buyer_response(false, Some(0.0), "rejected");

        let responses = vec![
            (&resp1, campaign1, None),
            (&resp2, campaign2, None),
            (&resp3, campaign3, None),
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should select the successful one with price
        assert_eq!(winner_id, campaign1);
    }

    #[test]
    fn test_select_winner_one_success_many_failures() {
        // One success among many failures
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();
        let campaign4 = Uuid::new_v4();

        let resp1 = create_buyer_response(false, None, "timeout");
        let resp2 = create_buyer_response(false, Some(0.0), "rejected");
        let resp3 = create_buyer_response(true, Some(200.0), "accepted");
        let resp4 = create_buyer_response(false, None, "error");

        let responses = vec![
            (&resp1, campaign1, None),
            (&resp2, campaign2, None),
            (&resp3, campaign3, None),
            (&resp4, campaign4, None),
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should select the successful one
        assert_eq!(winner_id, campaign3);
    }

    #[test]
    fn test_select_winner_same_price_different_priorities() {
        // Multiple campaigns with same price, different priorities
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(true, Some(100.0), "accepted");
        let resp3 = create_buyer_response(true, Some(100.0), "accepted");

        let responses = vec![
            (&resp1, campaign1, Some(3)), // Worst priority
            (&resp2, campaign2, Some(1)), // Best priority
            (&resp3, campaign3, Some(2)),
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should select by priority (lower number = higher priority)
        assert_eq!(winner_id, campaign2);
    }

    #[test]
    fn test_select_winner_price_precedence_over_priority() {
        // Higher price should win even with worse priority
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(200.0), "accepted");
        let resp2 = create_buyer_response(true, Some(100.0), "accepted");

        let responses = vec![
            (&resp1, campaign1, Some(5)), // Higher price, worse priority
            (&resp2, campaign2, Some(1)), // Lower price, better priority
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Price takes precedence
        assert_eq!(winner_id, campaign1);
    }

    #[test]
    fn test_select_winner_filters_zero_and_negative_prices() {
        // Zero and negative prices should be filtered out
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();
        let campaign3 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(true, Some(0.0), "accepted");
        let resp3 = create_buyer_response(true, Some(-10.0), "accepted");

        let responses = vec![
            (&resp1, campaign1, None),
            (&resp2, campaign2, None),
            (&resp3, campaign3, None),
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Only resp1 is valid
        assert_eq!(winner_id, campaign1);
    }

    #[test]
    fn test_select_winner_handles_none_priority() {
        // Campaigns without priority should still be selectable
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(150.0), "accepted");
        let resp2 = create_buyer_response(true, Some(100.0), "accepted");

        let responses = vec![(&resp1, campaign1, None), (&resp2, campaign2, None)];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should select highest price
        assert_eq!(winner_id, campaign1);
    }

    #[test]
    fn test_select_winner_epsilon_tolerance() {
        // Prices within 0.01 should be considered equal
        let campaign1 = Uuid::new_v4();
        let campaign2 = Uuid::new_v4();

        let resp1 = create_buyer_response(true, Some(100.0), "accepted");
        let resp2 = create_buyer_response(true, Some(100.009), "accepted");

        let responses = vec![
            (&resp1, campaign1, Some(1)), // Better priority
            (&resp2, campaign2, Some(2)), // Worse priority
        ];

        let (_winner, winner_id, _) = select_winner_for_test(responses);
        // Should select by priority since prices are within epsilon
        assert_eq!(winner_id, campaign1);
    }
}
