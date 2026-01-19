use criterion::{black_box, criterion_group, criterion_main, Criterion};
use leadsnebula_core::services::{
    buyer_router::BuyerResponse, ping_tree_router::select_winner_for_test,
};
use std::collections::HashMap;
use uuid::Uuid;

// Helper to create realistic buyer responses
fn create_buyer_response(
    success: bool,
    price: Option<f64>,
    status: &str,
    promise_id: Option<&str>,
) -> BuyerResponse {
    BuyerResponse {
        success,
        status: status.to_string(),
        error: if success {
            None
        } else {
            Some("Test error".to_string())
        },
        message: None,
        promise_id: promise_id.map(|s| s.to_string()),
        ping_id: Some(format!("PING_{}", Uuid::new_v4())),
        post_id: None,
        price,
        bid: None,
    }
}

// Create realistic campaign scenarios
fn create_mock_campaign_ids(count: usize) -> Vec<Uuid> {
    (0..count).map(|_| Uuid::new_v4()).collect()
}

fn create_priority_map(campaign_ids: &[Uuid]) -> HashMap<Uuid, Option<i32>> {
    campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, Some((i % 5 + 1) as i32))) // Priorities 1-5
        .collect()
}

// Benchmark: Small auction (10 campaigns) - typical case
fn benchmark_select_winner_10_campaigns(c: &mut Criterion) {
    let campaign_ids = create_mock_campaign_ids(10);
    let priority_map = create_priority_map(&campaign_ids);

    // Mix of success, rejection, and timeout responses
    // Store responses in a vector to maintain lifetimes
    let owned_responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let (success, price, status) = match i % 4 {
                0 => (true, Some(10.0 + i as f64), "accepted"), // 25% success
                1 => (true, Some(15.0 + i as f64), "accepted"), // 25% success (higher price)
                2 => (false, Some(0.0), "rejected"),            // 25% rejected
                _ => (false, None, "timeout"),                  // 25% timeout
            };
            let resp = create_buyer_response(success, price, status, Some("PROMISE"));
            let priority = priority_map.get(id).copied().flatten();
            (resp, *id, priority)
        })
        .collect();

    // Create references from owned vector for select_winner_for_test
    let responses: Vec<(&BuyerResponse, Uuid, Option<i32>)> = owned_responses
        .iter()
        .map(|(resp, id, pri)| (resp, *id, *pri))
        .collect();

    c.bench_function("select_winner_10_campaigns_mixed", |b| {
        b.iter(|| {
            // Use the actual select_winner_for_test function
            let winner = select_winner_for_test(responses.clone());
            black_box(winner)
        })
    });
}

// Benchmark: Medium auction (50 campaigns) - high load
fn benchmark_select_winner_50_campaigns(c: &mut Criterion) {
    let campaign_ids = create_mock_campaign_ids(50);
    let priority_map = create_priority_map(&campaign_ids);

    let owned_responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let (success, price, status) = match i % 5 {
                0 => (true, Some(20.0 + (i as f64 * 0.5)), "accepted"), // 20% success
                1 => (true, Some(25.0 + (i as f64 * 0.3)), "accepted"), // 20% success
                2 => (false, Some(0.0), "rejected"),                    // 20% rejected
                3 => (false, None, "timeout"),                          // 20% timeout
                _ => (true, Some(15.0 + (i as f64 * 0.2)), "accepted"), // 20% success (lower)
            };
            let resp = create_buyer_response(success, price, status, Some("PROMISE"));
            let priority = priority_map.get(id).copied().flatten();
            (resp, *id, priority)
        })
        .collect();

    let responses: Vec<(&BuyerResponse, Uuid, Option<i32>)> = owned_responses
        .iter()
        .map(|(resp, id, pri)| (resp, *id, *pri))
        .collect();

    c.bench_function("select_winner_50_campaigns_mixed", |b| {
        b.iter(|| {
            let winner = select_winner_for_test(responses.clone());
            black_box(winner)
        })
    });
}

// Benchmark: Price tie with epsilon tolerance (real-world edge case)
fn benchmark_select_winner_price_epsilon_tie(c: &mut Criterion) {
    let campaign_ids = create_mock_campaign_ids(20);
    let priority_map = create_priority_map(&campaign_ids);

    // Create responses with prices within PRICE_EPSILON (0.01) of each other
    let owned_responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            // Prices: 100.0, 100.005, 100.01, 100.015, etc.
            // First 5 should be considered equal (within 0.01 epsilon)
            let base_price = 100.0;
            let price = if i < 5 {
                base_price + (i as f64 * 0.005) // Within epsilon
            } else {
                base_price + (i as f64 * 0.1) // Outside epsilon
            };
            let resp = create_buyer_response(true, Some(price), "accepted", Some("PROMISE"));
            let priority = priority_map.get(id).copied().flatten();
            (resp, *id, priority)
        })
        .collect();

    let responses: Vec<(&BuyerResponse, Uuid, Option<i32>)> = owned_responses
        .iter()
        .map(|(resp, id, pri)| (resp, *id, *pri))
        .collect();

    c.bench_function("select_winner_price_epsilon_tie", |b| {
        b.iter(|| {
            let winner = select_winner_for_test(responses.clone());
            black_box(winner)
        })
    });
}

// Benchmark: All rejections (worst case - must return first error)
fn benchmark_select_winner_all_rejections(c: &mut Criterion) {
    let campaign_ids = create_mock_campaign_ids(30);
    let priority_map = create_priority_map(&campaign_ids);

    let owned_responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let status = if i % 2 == 0 { "rejected" } else { "timeout" };
            let resp = create_buyer_response(false, Some(0.0), status, None);
            let priority = priority_map.get(id).copied().flatten();
            (resp, *id, priority)
        })
        .collect();

    let responses: Vec<(&BuyerResponse, Uuid, Option<i32>)> = owned_responses
        .iter()
        .map(|(resp, id, pri)| (resp, *id, *pri))
        .collect();

    c.bench_function("select_winner_all_rejections", |b| {
        b.iter(|| {
            let winner = select_winner_for_test(responses.clone());
            black_box(winner)
        })
    });
}

// Benchmark: Priority tie-breaker (same price, different priorities)
fn benchmark_select_winner_priority_tie(c: &mut Criterion) {
    let campaign_ids = create_mock_campaign_ids(15);

    // All same price, different priorities
    let owned_responses: Vec<(BuyerResponse, Uuid, Option<i32>)> = campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let resp = create_buyer_response(true, Some(100.0), "accepted", Some("PROMISE"));
            let priority = Some((i % 5 + 1) as i32); // Priorities 1-5, cycling
            (resp, *id, priority)
        })
        .collect();

    let responses: Vec<(&BuyerResponse, Uuid, Option<i32>)> = owned_responses
        .iter()
        .map(|(resp, id, pri)| (resp, *id, *pri))
        .collect();

    c.bench_function("select_winner_priority_tie", |b| {
        b.iter(|| {
            let winner = select_winner_for_test(responses.clone());
            black_box(winner)
        })
    });
}

criterion_group!(
    benches,
    benchmark_select_winner_10_campaigns,
    benchmark_select_winner_50_campaigns,
    benchmark_select_winner_price_epsilon_tie,
    benchmark_select_winner_all_rejections,
    benchmark_select_winner_priority_tie
);
criterion_main!(benches);
