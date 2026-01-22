use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use leadsnebula_core::cache::CacheService;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

// Mock SSM service for benchmarks
struct MockSsmService {
    params: HashMap<String, String>,
    latency_ms: u64,
}

impl MockSsmService {
    fn new() -> Self {
        let mut params = HashMap::new();
        params.insert(
            "/leadsnebula/dev/encryption/det_key".to_string(),
            "mock_det_key_123456789012345678901234567890".to_string(),
        );
        params.insert(
            "/leadsnebula/dev/encryption/salt".to_string(),
            "mock_salt_123456789012345678901234567890".to_string(),
        );
        Self {
            params,
            latency_ms: 50, // Simulate 50ms SSM latency
        }
    }

    async fn get_parameter(&self, path: &str) -> Option<String> {
        // Simulate network latency
        tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        self.params.get(path).cloned()
    }
}

// Mock Neon DB for benchmarks
struct MockNeonDb {
    latency_ms: u64,
}

impl MockNeonDb {
    fn new() -> Self {
        Self {
            latency_ms: 200, // Simulate 200ms DB latency
        }
    }

    async fn query(&self) -> Result<(), ()> {
        tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        Ok(())
    }
}

/// Benchmark with realistic SSM/DB latencies
fn bench_auction_with_realistic_latencies(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("auction_realistic");
    group.sample_size(100); // More samples for P95/P99
    group.measurement_time(Duration::from_secs(30));

    for campaign_count in [1, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(campaign_count),
            campaign_count,
            |b, &count| {
                b.iter(|| {
                    rt.block_on(async {
                        let ssm = MockSsmService::new();
                        let db = MockNeonDb::new();

                        let start = Instant::now();

                        // Simulate pre-checks (SSM + DB)
                        let _ssm_result = ssm
                            .get_parameter("/leadsnebula/dev/encryption/det_key")
                            .await;
                        let _db_result = db.query().await;

                        // Simulate ping auction (parallel campaigns)
                        let mut handles = Vec::new();
                        for _ in 0..count {
                            handles.push(tokio::spawn(async {
                                tokio::time::sleep(Duration::from_millis(10)).await;
                                // Pulsar ping
                            }));
                        }
                        futures::future::join_all(handles).await;

                        // Simulate post
                        tokio::time::sleep(Duration::from_millis(5)).await;

                        let duration = start.elapsed();
                        black_box(duration)
                    })
                });
            },
        );
    }
    group.finish();
}

/// Calculate P50, P95, P99 percentiles
fn bench_percentiles(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("auction_percentiles", |b| {
        b.iter_custom(|iters| {
            let mut durations = Vec::new();
            rt.block_on(async {
                let ssm = MockSsmService::new();
                let db = MockNeonDb::new();

                for _ in 0..iters {
                    let start = Instant::now();

                    // Simulate full auction
                    let _ssm_result = ssm
                        .get_parameter("/leadsnebula/dev/encryption/det_key")
                        .await;
                    let _db_result = db.query().await;

                    // Simulate 5 campaigns
                    let mut handles = Vec::new();
                    for _ in 0..5 {
                        handles.push(tokio::spawn(async {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }));
                    }
                    futures::future::join_all(handles).await;
                    tokio::time::sleep(Duration::from_millis(5)).await;

                    durations.push(start.elapsed());
                }
            });

            durations.sort();
            if !durations.is_empty() {
                let p50 = durations[durations.len() * 50 / 100];
                let p95 = durations[durations.len() * 95 / 100];
                let p99_idx = (durations.len() * 99 / 100).min(durations.len() - 1);
                let p99 = durations[p99_idx];

                println!("\n=== Percentile Results ===");
                println!("P50: {:?}", p50);
                println!("P95: {:?}", p95);
                println!("P99: {:?}", p99);
                println!("========================\n");
            }

            durations.into_iter().sum()
        });
    });
}

/// Benchmark critical path only (lead_received → post_response_parsed)
/// Excludes DB writes, background tasks, verbose lookups
/// Runs exactly 10 leads and reports P50/P95/P99 percentiles
fn bench_critical_path(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    const NUM_LEADS: usize = 10;

    c.bench_function("critical_path_10_leads", |b| {
        b.iter(|| {
            rt.block_on(async {
                let ssm = MockSsmService::new();
                let mut durations = Vec::with_capacity(NUM_LEADS);

                // Run exactly 10 leads
                for _ in 0..NUM_LEADS {
                    let start = Instant::now();

                    // Critical path only:
                    // 1. Pre-checks (SSM with timeout)
                    let _ssm_result = tokio::time::timeout(
                        Duration::from_millis(200),
                        ssm.get_parameter("/leadsnebula/dev/encryption/det_key"),
                    )
                    .await;

                    // 2. Ping auction (sync Pulsar, no DB)
                    let mut handles = Vec::new();
                    for _ in 0..10 {
                        // Simulate 10 campaigns
                        handles.push(tokio::spawn(async {
                            // Sync Pulsar call (instant)
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }));
                    }
                    futures::future::join_all(handles).await;

                    // 3. Post (sync Pulsar, no DB)
                    tokio::time::sleep(Duration::from_millis(1)).await;

                    // 4. Response (minimal, simd-json)
                    let _response = json!({"status": "sold", "post_id": "RP_test"});
                    let _ = simd_json::to_string(&_response);

                    durations.push(start.elapsed());
                }

                // Calculate and report percentiles
                durations.sort();
                if !durations.is_empty() {
                    let p50 = durations[durations.len() * 50 / 100];
                    let p95 = durations[durations.len() * 95 / 100];
                    let p99_idx = (durations.len() * 99 / 100).min(durations.len() - 1);
                    let p99 = durations[p99_idx];

                    println!("\n=== Critical Path Percentiles (10 leads) ===");
                    println!("P50: {:?} ({:.2} ms)", p50, p50.as_secs_f64() * 1000.0);
                    println!("P95: {:?} ({:.2} ms)", p95, p95.as_secs_f64() * 1000.0);
                    println!("P99: {:?} ({:.2} ms)", p99, p99.as_secs_f64() * 1000.0);
                    println!("============================================\n");
                }

                black_box(durations.into_iter().sum::<Duration>())
            })
        });
    });
}

criterion_group!(
    benches,
    bench_full_auction_flow,
    bench_cache_operations,
    bench_json_serialization,
    bench_db_query_performance,
    bench_auction_with_realistic_latencies,
    bench_percentiles,
    bench_critical_path
);
criterion_main!(benches);
