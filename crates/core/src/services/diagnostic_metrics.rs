use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Diagnostic metrics for tracking database queries and cache operations
/// Used to establish baseline metrics before optimizations
#[derive(Debug, Clone)]
pub struct DiagnosticMetrics {
    db_queries: Arc<AtomicU32>,
    db_query_time_ms: Arc<AtomicU64>,
    cache_hits: Arc<AtomicU32>,
    cache_misses: Arc<AtomicU32>,
}

impl DiagnosticMetrics {
    pub fn new() -> Self {
        Self {
            db_queries: Arc::new(AtomicU32::new(0)),
            db_query_time_ms: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU32::new(0)),
            cache_misses: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn record_query(&self, duration_ms: u64) {
        self.db_queries.fetch_add(1, Ordering::Relaxed);
        self.db_query_time_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_query_count(&self) -> u32 {
        self.db_queries.load(Ordering::Relaxed)
    }

    pub fn get_total_query_time_ms(&self) -> u64 {
        self.db_query_time_ms.load(Ordering::Relaxed)
    }

    pub fn get_cache_hits(&self) -> u32 {
        self.cache_hits.load(Ordering::Relaxed)
    }

    pub fn get_cache_misses(&self) -> u32 {
        self.cache_misses.load(Ordering::Relaxed)
    }

    pub fn get_cache_hit_rate(&self) -> f64 {
        let hits = self.get_cache_hits();
        let misses = self.get_cache_misses();
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64) / (total as f64) * 100.0
        }
    }

    pub fn log_summary(&self, context: &str) {
        let queries = self.get_query_count();
        let total_time_ms = self.get_total_query_time_ms();
        let avg_time_ms = if queries > 0 {
            total_time_ms as f64 / queries as f64
        } else {
            0.0
        };
        let cache_hits = self.get_cache_hits();
        let cache_misses = self.get_cache_misses();
        let hit_rate = self.get_cache_hit_rate();

        tracing::info!(
            "Diagnostic metrics [{}]: queries={}, total_time_ms={}, avg_time_ms={:.2}, cache_hits={}, cache_misses={}, cache_hit_rate={:.2}%",
            context,
            queries,
            total_time_ms,
            avg_time_ms,
            cache_hits,
            cache_misses,
            hit_rate
        );
    }

    pub fn reset(&self) {
        self.db_queries.store(0, Ordering::Relaxed);
        self.db_query_time_ms.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
    }
}

impl Default for DiagnosticMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper macro to wrap database queries with timing
#[macro_export]
macro_rules! timed_query {
    ($metrics:expr, $query_future:expr) => {{
        let start = std::time::Instant::now();
        let result = $query_future.await;
        let duration_ms = start.elapsed().as_millis() as u64;
        if let Some(ref m) = $metrics {
            m.record_query(duration_ms);
        }
        tracing::debug!("DB query: duration={}ms", duration_ms);
        result
    }};
}
