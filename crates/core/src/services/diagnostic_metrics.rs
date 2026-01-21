use serde_json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub struct DiagnosticMetrics {
    db_queries: AtomicU64,
    db_query_time_ms: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    ping_auction_time_ms: AtomicU64,
    ping_auction_count: AtomicU64,
    post_time_ms: AtomicU64,
    post_count: AtomicU64,
    // Per-stage timing (for percentile calculations)
    stage_timings: Mutex<HashMap<String, Vec<u64>>>,
}

impl Default for DiagnosticMetrics {
    fn default() -> Self {
        Self {
            db_queries: AtomicU64::new(0),
            db_query_time_ms: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            ping_auction_time_ms: AtomicU64::new(0),
            ping_auction_count: AtomicU64::new(0),
            post_time_ms: AtomicU64::new(0),
            post_count: AtomicU64::new(0),
            stage_timings: Mutex::new(HashMap::new()),
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

    pub fn record_ping_auction(&self, duration_ms: u64) {
        self.ping_auction_count.fetch_add(1, Ordering::Relaxed);
        self.ping_auction_time_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
    }

    pub fn record_post(&self, duration_ms: u64) {
        self.post_count.fetch_add(1, Ordering::Relaxed);
        self.post_time_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }

    pub fn record_stage_timing(&self, stage_name: &str, duration_ms: u64) {
        let mut timings = self.stage_timings.lock().unwrap();
        timings
            .entry(stage_name.to_string())
            .or_default()
            .push(duration_ms);
        // Keep only last 1000 timings per stage to prevent memory growth
        if let Some(times) = timings.get_mut(stage_name) {
            if times.len() > 1000 {
                times.drain(0..times.len() - 1000);
            }
        }
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

    pub fn get_avg_ping_auction_time_ms(&self) -> f64 {
        let count = self.ping_auction_count.load(Ordering::Relaxed);
        let total = self.ping_auction_time_ms.load(Ordering::Relaxed);
        if count == 0 {
            0.0
        } else {
            total as f64 / count as f64
        }
    }

    pub fn get_avg_post_time_ms(&self) -> f64 {
        let count = self.post_count.load(Ordering::Relaxed);
        let total = self.post_time_ms.load(Ordering::Relaxed);
        if count == 0 {
            0.0
        } else {
            total as f64 / count as f64
        }
    }

    pub fn get_stage_percentiles(&self, stage_name: &str) -> Option<(u64, u64, u64)> {
        let timings = self.stage_timings.lock().unwrap();
        let times = timings.get(stage_name)?;
        if times.is_empty() {
            return None;
        }
        let mut sorted = times.clone();
        sorted.sort();
        let p50 = sorted[sorted.len() * 50 / 100];
        let p95 = sorted[sorted.len() * 95 / 100];
        let p99 = sorted[sorted.len() * 99 / 100.min(sorted.len() - 1)];
        Some((p50, p95, p99))
    }

    pub fn log_summary(&self, context: &str) -> serde_json::Value {
        let db_queries = self.get_query_count();
        let db_query_time_ms = self.get_total_query_time_ms();
        let cache_hits = self.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.cache_misses.load(Ordering::Relaxed);
        let cache_hit_rate = self.get_cache_hit_rate();
        let avg_ping_auction = self.get_avg_ping_auction_time_ms();
        let avg_post = self.get_avg_post_time_ms();

        let summary = serde_json::json!({
            "context": context,
            "db_queries": db_queries,
            "db_query_time_ms": db_query_time_ms,
            "cache_hits": cache_hits,
            "cache_misses": cache_misses,
            "cache_hit_rate": format!("{:.1}%", cache_hit_rate),
            "avg_ping_auction_ms": format!("{:.1}", avg_ping_auction),
            "avg_post_ms": format!("{:.1}", avg_post)
        });

        // Use structured logging with key-value pairs
        tracing::info!(
            context = %context,
            db_queries = db_queries,
            db_query_time_ms = db_query_time_ms,
            cache_hits = cache_hits,
            cache_misses = cache_misses,
            cache_hit_rate = %format!("{:.1}%", cache_hit_rate),
            avg_ping_auction_ms = %format!("{:.1}", avg_ping_auction),
            avg_post_ms = %format!("{:.1}", avg_post),
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
