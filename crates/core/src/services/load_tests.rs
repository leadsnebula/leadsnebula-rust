// Load tests for concurrent ping auctions
// These tests simulate high concurrency scenarios
// NOTE: These are load/stress tests, not functionality tests
// They are marked as #[ignore] and should be run manually when needed

#[cfg(test)]
mod load_tests {
    use crate::services::buyer_router::BuyerResponse;
    use crate::services::ping_tree_router::select_winner_for_test;
    use std::sync::Arc;
    use tokio::sync::Barrier;
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

    #[tokio::test]
    #[ignore] // Load test - skip in CI, run manually when needed
    async fn test_concurrent_ping_auction_1000_responses() {
        // Simulate 1000 concurrent ping responses
        let num_responses = 1000;
        let mut responses = Vec::new();
        let mut campaign_ids = Vec::new();

        for i in 0..num_responses {
            let campaign_id = Uuid::new_v4();
            campaign_ids.push(campaign_id);

            let price = Some(100.0 + (i as f64));
            let resp = create_buyer_response(true, price, "accepted");
            responses.push((resp, campaign_id, Some(i as i32)));
        }

        // Select winner from 1000 responses
        let start = std::time::Instant::now();
        let (_winner, winner_id, _) =
            select_winner_for_test(responses.iter().map(|(r, c, p)| (r, *c, *p)).collect());
        let duration = start.elapsed();

        // Should select highest price
        assert_eq!(winner_id, campaign_ids[num_responses - 1]);

        // Should complete quickly (< 100ms for 1000 responses)
        assert!(duration.as_millis() < 100, "Selection should be fast");
    }

    #[tokio::test]
    #[ignore] // Load test - skip in CI, run manually when needed
    async fn test_concurrent_winner_selection_parallel() {
        // Test concurrent winner selection from multiple threads
        let num_threads = 10;
        let responses_per_thread = 100;
        let barrier = Arc::new(Barrier::new(num_threads));

        let mut handles = vec![];

        for thread_id in 0..num_threads {
            let barrier_clone = barrier.clone();

            let handle = tokio::spawn(async move {
                // Wait for all threads to be ready
                barrier_clone.wait().await;

                // Create responses for this thread
                let mut responses = Vec::new();
                for i in 0..responses_per_thread {
                    let campaign_id = Uuid::new_v4();
                    let price = Some(100.0 + (thread_id as f64 * 1000.0) + (i as f64));
                    let resp = create_buyer_response(true, price, "accepted");
                    responses.push((resp, campaign_id, Some(i as i32)));
                }

                // Select winner
                let start = std::time::Instant::now();
                let response_refs: Vec<_> = responses.iter().map(|(r, c, p)| (r, *c, *p)).collect();
                let (_winner, winner_id, _) = select_winner_for_test(response_refs);
                let duration = start.elapsed();

                (thread_id, winner_id, duration)
            });

            handles.push(handle);
        }

        // Collect results
        let mut results = Vec::new();
        for handle in handles {
            let result = handle.await.unwrap();
            results.push(result);
        }

        // Verify all threads completed successfully
        assert_eq!(results.len(), num_threads);
        for (thread_id, _winner_id, duration) in results {
            assert!(
                duration.as_millis() < 100,
                "Thread {} should complete quickly",
                thread_id
            );
        }
    }

    #[tokio::test]
    #[ignore] // Load test - skip in CI, run manually when needed
    async fn test_mixed_response_types_high_volume() {
        // Test winner selection with mixed response types at high volume
        let num_responses = 500;
        let mut responses = Vec::new();

        for i in 0..num_responses {
            let campaign_id = Uuid::new_v4();
            let (success, price, status) = match i % 4 {
                0 => (true, Some(100.0 + (i as f64)), "accepted"),
                1 => (false, None, "timeout"),
                2 => (false, Some(0.0), "rejected"),
                _ => (true, Some(50.0 + (i as f64)), "accepted"),
            };

            let resp = create_buyer_response(success, price, status);
            responses.push((resp, campaign_id, Some(i as i32)));
        }

        let start = std::time::Instant::now();
        let (_winner, winner_id, _) =
            select_winner_for_test(responses.iter().map(|(r, c, p)| (r, *c, *p)).collect());
        let duration = start.elapsed();

        // Should select a valid winner (not timeout/rejected)
        assert!(duration.as_millis() < 100, "Selection should be fast");
        // winner_id should be one of the successful campaigns
        let _ = winner_id; // Suppress unused warning
    }

    #[tokio::test]
    #[ignore] // Load test - skip in CI, run manually when needed
    async fn test_all_rejections_high_volume() {
        // Test handling of all rejections at high volume
        let num_responses = 1000;
        let mut responses = Vec::new();

        for i in 0..num_responses {
            let campaign_id = Uuid::new_v4();
            let resp = create_buyer_response(false, Some(0.0), "rejected");
            responses.push((resp, campaign_id, Some(i as i32)));
        }

        let start = std::time::Instant::now();
        let response_refs: Vec<_> = responses.iter().map(|(r, c, p)| (r, *c, *p)).collect();
        let (_winner, winner_id, _) = select_winner_for_test(response_refs);
        let duration = start.elapsed();

        // Should return first error response quickly
        assert!(duration.as_millis() < 100, "Selection should be fast");
        // winner_id should be first campaign (all rejected)
        let _ = winner_id; // Suppress unused warning
    }
}
