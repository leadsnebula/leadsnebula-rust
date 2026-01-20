use serde_json;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct DiagnosticMetrics {
    db_queries: AtomicU64,
    db_query_time_ms: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
}

impl Default for DiagnosticMetrics {
    fn default() -> Self {
        Self {
            db_queries: AtomicU64::new(0),
            db_query_time_ms: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
        }
    }
}

impl DiagnosticMetrics {
    pub fn new() -> Self {
        Self::default()
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

    pub fn get_query_count(&self) -> u64 {
        self.db_queries.load(Ordering::Relaxed)
    }

    pub fn get_total_query_time_ms(&self) -> u64 {
        self.db_query_time_ms.load(Ordering::Relaxed)
    }

    pub fn get_cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64) / (total as f64) * 100.0
        }
    }

    pub fn log_summary(&self, context: &str) -> serde_json::Value {
        let db_queries = self.get_query_count();
        let db_query_time_ms = self.get_total_query_time_ms();
        let cache_hits = self.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.cache_misses.load(Ordering::Relaxed);
        let cache_hit_rate = self.get_cache_hit_rate();

        let summary = serde_json::json!({
            "context": context,
            "db_queries": db_queries,
            "db_query_time_ms": db_query_time_ms,
            "cache_hits": cache_hits,
            "cache_misses": cache_misses,
            "cache_hit_rate": format!("{:.1}%", cache_hit_rate)
        });

        // Use structured logging with key-value pairs
        tracing::info!(
            context = %context,
            db_queries = db_queries,
            db_query_time_ms = db_query_time_ms,
            cache_hits = cache_hits,
            cache_misses = cache_misses,
            cache_hit_rate = %format!("{:.1}%", cache_hit_rate),
            "Diagnostic metrics summary"
        );

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let metrics = DiagnosticMetrics::new();
        assert_eq!(metrics.get_query_count(), 0);
        assert_eq!(metrics.get_total_query_time_ms(), 0);
        assert_eq!(metrics.get_cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_record_query() {
        let metrics = DiagnosticMetrics::new();
        metrics.record_query(100);
        assert_eq!(metrics.get_query_count(), 1);
        assert_eq!(metrics.get_total_query_time_ms(), 100);
    }

    #[test]
    fn test_record_cache_hit_miss() {
        let metrics = DiagnosticMetrics::new();
        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();
        let hit_rate = metrics.get_cache_hit_rate();
        // 2 hits out of 3 total = 66.666...%
        assert!((hit_rate - 66.66666666666667).abs() < 0.0001);
    }

    #[test]
    fn test_log_summary() {
        let metrics = DiagnosticMetrics::new();
        metrics.record_query(50);
        metrics.record_cache_hit();
        let summary = metrics.log_summary("test");
        assert!(summary.get("context").is_some());
        assert!(summary.get("db_queries").is_some());
    }
}
