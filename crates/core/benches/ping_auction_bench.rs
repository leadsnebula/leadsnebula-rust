use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use uuid::Uuid;

// Create mock campaign IDs and priority map for benchmarking
fn create_mock_campaign_ids(count: usize) -> Vec<Uuid> {
    (0..count).map(|_| Uuid::new_v4()).collect()
}

fn create_priority_map(campaign_ids: &[Uuid]) -> HashMap<Uuid, Option<i32>> {
    campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, Some(i as i32)))
        .collect()
}

// Benchmark select_winner function (if it's public or we can access it)
// Note: This is a simplified benchmark that focuses on the winner selection logic
// Full integration benchmarks would require database setup
fn benchmark_winner_selection(c: &mut Criterion) {
    let campaign_ids = create_mock_campaign_ids(10);
    let priority_map = create_priority_map(&campaign_ids);

    // Create mock buyer responses
    let responses: Vec<(
        leadsnebula_core::services::buyer_router::BuyerResponse,
        Uuid,
        Option<i32>,
    )> = campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            (
                leadsnebula_core::services::buyer_router::BuyerResponse {
                    success: true,
                    status: "accepted".to_string(),
                    error: None,
                    message: None,
                    promise_id: Some(format!("PROMISE_{}", i)),
                    ping_id: Some(format!("PING_{}", i)),
                    post_id: None,
                    price: Some(10.0 + i as f64),
                },
                *id,
                priority_map.get(id).copied().flatten(),
            )
        })
        .collect();

    c.bench_function("select_winner_10_campaigns", |b| {
        b.iter(|| {
            // Clone responses for each iteration to avoid mutating the original
            let mut responses_clone = responses.clone();
            // Simulate winner selection logic
            // Sort by price descending, then by priority ascending
            responses_clone.sort_by(|a, b| {
                let price_a = a.0.price.unwrap_or(0.0);
                let price_b = b.0.price.unwrap_or(0.0);
                match price_b.partial_cmp(&price_a) {
                    Some(std::cmp::Ordering::Equal) => {
                        let pri_a = a.2.unwrap_or(i32::MAX);
                        let pri_b = b.2.unwrap_or(i32::MAX);
                        pri_a.cmp(&pri_b)
                    }
                    Some(ord) => ord,
                    None => std::cmp::Ordering::Equal,
                }
            });
            black_box(responses_clone[0].clone())
        })
    });
}

fn benchmark_winner_selection_50_campaigns(c: &mut Criterion) {
    let campaign_ids = create_mock_campaign_ids(50);
    let priority_map = create_priority_map(&campaign_ids);

    let responses: Vec<(
        leadsnebula_core::services::buyer_router::BuyerResponse,
        Uuid,
        Option<i32>,
    )> = campaign_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            (
                leadsnebula_core::services::buyer_router::BuyerResponse {
                    success: true,
                    status: "accepted".to_string(),
                    error: None,
                    message: None,
                    promise_id: Some(format!("PROMISE_{}", i)),
                    ping_id: Some(format!("PING_{}", i)),
                    post_id: None,
                    price: Some(10.0 + i as f64),
                },
                *id,
                priority_map.get(id).copied().flatten(),
            )
        })
        .collect();

    c.bench_function("select_winner_50_campaigns", |b| {
        b.iter(|| {
            // Clone responses for each iteration to avoid mutating the original
            let mut responses_clone = responses.clone();
            responses_clone.sort_by(|a, b| {
                let price_a = a.0.price.unwrap_or(0.0);
                let price_b = b.0.price.unwrap_or(0.0);
                match price_b.partial_cmp(&price_a) {
                    Some(std::cmp::Ordering::Equal) => {
                        let pri_a = a.2.unwrap_or(i32::MAX);
                        let pri_b = b.2.unwrap_or(i32::MAX);
                        pri_a.cmp(&pri_b)
                    }
                    Some(ord) => ord,
                    None => std::cmp::Ordering::Equal,
                }
            });
            black_box(responses_clone[0].clone())
        })
    });
}

criterion_group!(
    benches,
    benchmark_winner_selection,
    benchmark_winner_selection_50_campaigns
);
criterion_main!(benches);
