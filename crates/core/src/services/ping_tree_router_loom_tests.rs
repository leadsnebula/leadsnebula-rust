// Loom concurrency tests for ping auction
// These tests use loom to explore all possible thread interleavings

#[cfg(test)]
mod loom_tests {
    use crate::services::buyer_router::BuyerResponse;
    use crate::services::ping_tree_router::select_winner_for_test;
    use loom::sync::Arc;
    use loom::thread;
    use std::sync::atomic::{AtomicU64, Ordering};
    use uuid::Uuid;

    #[test]
    fn test_concurrent_ping_responses() {
        loom::model(|| {
            let responses = Arc::new(AtomicU64::new(0));
            let responses_clone = Arc::clone(&responses);

            // Simulate concurrent buyer responses
            let t1 = thread::spawn(move || {
                responses_clone.fetch_add(1, Ordering::SeqCst);
            });

            let responses_clone2 = Arc::clone(&responses);
            let t2 = thread::spawn(move || {
                responses_clone2.fetch_add(1, Ordering::SeqCst);
            });

            t1.join().unwrap();
            t2.join().unwrap();

            // Both threads should have incremented
            assert_eq!(responses.load(Ordering::SeqCst), 2);
        });
    }

    #[test]
    fn test_winner_selection_race_condition() {
        loom::model(|| {
            // Create test responses
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
                price: Some(150.0),
                bid: Some(150.0), // Set bid for ping auction
            };

            let campaign1 = Uuid::new_v4();
            let campaign2 = Uuid::new_v4();

            let responses = vec![(&resp1, campaign1, None), (&resp2, campaign2, None)];

            // Test winner selection is deterministic
            let (winner1, winner_id1, _) = select_winner_for_test(responses.clone());
            let (winner2, winner_id2, _) = select_winner_for_test(responses);

            // Should always select highest price
            assert_eq!(winner_id1, campaign2);
            assert_eq!(winner_id2, campaign2);
            assert_eq!(winner1.price, Some(150.0));
            assert_eq!(winner2.price, Some(150.0));
        });
    }

    #[test]
    fn test_concurrent_price_updates() {
        loom::model(|| {
            let price = Arc::new(AtomicU64::new(100));
            let price_clone1 = Arc::clone(&price);
            let price_clone2 = Arc::clone(&price);

            // Simulate concurrent price updates
            let t1 = thread::spawn(move || {
                let current = price_clone1.load(Ordering::SeqCst);
                price_clone1.store(current + 10, Ordering::SeqCst);
            });

            let t2 = thread::spawn(move || {
                let current = price_clone2.load(Ordering::SeqCst);
                price_clone2.store(current + 20, Ordering::SeqCst);
            });

            t1.join().unwrap();
            t2.join().unwrap();

            // Final price should be one of the possible outcomes
            let final_price = price.load(Ordering::SeqCst);
            assert!(final_price == 130 || final_price == 120 || final_price == 110);
        });
    }
}
