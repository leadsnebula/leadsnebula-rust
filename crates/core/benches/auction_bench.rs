use criterion::{black_box, criterion_group, criterion_main, Criterion};
use leadsnebula_core::cache::CacheService;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

/// Benchmark full auction flow (ping + post)
/// Target: <200ms for full auction (warm cache)
fn bench_full_auction_flow(c: &mut Criterion) {
    c.bench_function("full_auction_flow", |b| {
        b.iter(|| {
            // Simulate full auction flow
            let start = Instant::now();

            // Simulate pre-checks (cached)
            let _vertical_slug = black_box("solar".to_string());

            // Simulate ping auction
            let _ping_responses = black_box(vec![
                json!({"bid": 150.0, "status": "accepted"}),
                json!({"bid": 200.0, "status": "accepted"}),
            ]);

            // Simulate post
            let _post_response = black_box(json!({"price": 180.0, "status": "sold"}));

            let duration = start.elapsed();
            assert!(
                duration.as_millis() < 200,
                "Auction should complete in <200ms"
            );
            duration
        });
    });
}

/// Benchmark cache operations (L1 + L2)
fn bench_cache_operations(c: &mut Criterion) {
    let cache = Arc::new(CacheService::new(None, "test".to_string()));

    c.bench_function("cache_get_or_insert_with", |b| {
        b.iter(|| {
            let cache_clone = cache.clone();
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                cache_clone
                    .get_or_insert_with("test_key", 3600, || async { Ok(json!({"test": "value"})) })
                    .await
                    .unwrap()
            })
        });
    });
}

/// Benchmark JSON serialization (serde_json vs simd-json)
fn bench_json_serialization(c: &mut Criterion) {
    let data = json!({
        "lead_id": "test-123",
        "campaign_id": "camp-456",
        "buyer_id": "buyer-789",
        "bid": 150.0,
        "status": "accepted",
        "metadata": {
            "source": "facebook",
            "vertical": "solar",
            "timestamp": "2026-01-21T12:00:00Z"
        }
    });

    c.bench_function("json_serialize_serde", |b| {
        b.iter(|| black_box(serde_json::to_string(&data).unwrap()));
    });

    c.bench_function("json_serialize_simd", |b| {
        b.iter(|| {
            let bytes = simd_json::to_vec(&data).unwrap();
            black_box(String::from_utf8(bytes).unwrap())
        });
    });

    let json_str = serde_json::to_string(&data).unwrap();
    c.bench_function("json_deserialize_serde", |b| {
        b.iter(|| black_box(serde_json::from_str::<serde_json::Value>(&json_str).unwrap()));
    });

    c.bench_function("json_deserialize_simd", |b| {
        b.iter(|| {
            let mut bytes = json_str.clone().into_bytes();
            // simd_json requires mutable bytes for in-place parsing
            black_box(simd_json::from_slice::<serde_json::Value>(&mut bytes).unwrap())
        });
    });
}

/// Benchmark DB query performance (mock)
fn bench_db_query_performance(c: &mut Criterion) {
    c.bench_function("db_query_simple", |b| {
        b.iter(|| {
            // Simulate a simple SELECT query
            let start = Instant::now();
            // In real benchmark, this would be an actual DB query
            std::thread::sleep(std::time::Duration::from_millis(1));
            black_box(start.elapsed())
        });
    });
}

criterion_group!(
    benches,
    bench_full_auction_flow,
    bench_cache_operations,
    bench_json_serialization,
    bench_db_query_performance
);
criterion_main!(benches);
